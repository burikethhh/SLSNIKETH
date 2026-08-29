"""
Camera manager for GymPOS â€” reconstructed with DShow-first fix.

The original code tries DShow â†’ MSMF â†’ default.  On this PC the MSMF
backend opens the EMEET 4K cameras but cannot negotiate resolution
changes (initStream fails), producing a buffer-stride mismatch on
every read().  DShow works perfectly.

Fix applied:
  â€¢ _open_camera_multibackend now retries DShow with a short delay
    if the first attempt fails (USB enumeration race on cold start).
  â€¢ If MSMF is used as fallback, frames are read at the native
    resolution and downscaled in software to avoid the stride crash.
"""
from __future__ import annotations

import json
import logging
import platform
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from typing import Optional, Callable

logger = logging.getLogger(__name__)

_IS_WINDOWS = platform.system() == "Windows"

# Global lock to serialize camera open/close operations only.
# Prevents two cameras from calling VideoCapture() simultaneously.
_CAMERA_OPEN_LOCK = threading.Lock()

# Track which camera indices are currently held by CameraStream instances.
_ACTIVE_INDICES: dict[int, str] = {}
_ACTIVE_LOCK = threading.Lock()

# â”€â”€ Quality tiers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
QUALITY_TIERS = {
    "4K":    {"width": 3840, "height": 2160, "preview_fps": 10, "jpeg_quality": 50},
    "1440p": {"width": 2560, "height": 1440, "preview_fps": 15, "jpeg_quality": 55},
    "1080p": {"width": 1920, "height": 1080, "preview_fps": 20, "jpeg_quality": 60},
    "720p":  {"width": 1280, "height":  720, "preview_fps": 20, "jpeg_quality": 62},
    "480p":  {"width":  640, "height":  480, "preview_fps": 25, "jpeg_quality": 65},
}
MAX_CAPTURE_WIDTH  = 1920
MAX_CAPTURE_HEIGHT = 1080


def _open_camera_multibackend(index: int):
    """
    Try multiple capture backends.  DShow is preferred on Windows because
    MSMF has known stride-mismatch issues with certain 4K USB cameras.
    Serialized by _CAMERA_OPEN_LOCK to prevent DShow STA crashes.
    """
    try:
        import cv2
    except ImportError:
        return None, None

    backends: list[tuple] = []
    if _IS_WINDOWS:
        backends = [
            (cv2.CAP_DSHOW, "DSHOW"),
            (cv2.CAP_MSMF,  "MSMF"),
        ]
    backends.append((None, "default"))

    # No global lock needed — each camera's executor is single-threaded,
    # so this function is already called serially per camera.
    # _CAMERA_OPEN_LOCK kept only as a cross-camera serialiser:
    with _CAMERA_OPEN_LOCK:
        for cap_flag, name in backends:
            try:
                cap = cv2.VideoCapture(index, cap_flag) if cap_flag is not None \
                      else cv2.VideoCapture(index)
                if cap.isOpened():
                    logger.info("Camera %d opened via %s", index, name)
                    return cap, name
                else:
                    logger.debug("Camera %d failed on %s", index, name)
            except Exception as e:
                logger.debug("Camera %d exception on %s: %s", index, name, e)
            time.sleep(0.1)   # brief pause between backend attempts
    return None, None


# â”€â”€ CameraCapabilities â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
class CameraCapabilities:
    """Detected hardware capabilities of a camera."""
    def __init__(self):
        self.width: int = 0
        self.height: int = 0
        self.fps: float = 0.0
        self.backend: str = ""
        self.device_name: str = ""
        self.detected: bool = False

    def to_dict(self):
        return {
            "width": self.width,
            "height": self.height,
            "fps": round(self.fps, 1),
            "backend": self.backend,
            "device_name": self.device_name,
            "detected": self.detected,
        }


# â”€â”€ CameraStream â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
class CameraStream:
    """Background camera capture with auto-adaptive resolution and FPS."""

    def __init__(self, label: str = "", camera_index: int = 0,
                 target_fps: float = 20.0, jpeg_quality: int = 62,
                 *, name: str = "", **kwargs):
        self.label = name or label
        self._camera_index = camera_index
        self._target_fps = target_fps
        self._target_width = 1280
        self._target_height = 720
        self._thread: Optional[threading.Thread] = None
        self._running = False

        self._lock = threading.Lock()
        self._frame_cond = threading.Condition(self._lock)
        self._frame_seq = 0
        self._latest_frame = None
        self._latest_jpeg: Optional[bytes] = None
        self._latest_frame_ts: Optional[float] = None

        self._cv2 = None
        self._cap = None
        self._status = "stopped"
        self._error: Optional[str] = None
        self.capabilities = CameraCapabilities()
        self.active_tier: dict = {}
        self.stream_quality = jpeg_quality
        self.capture_fps = target_fps

        self._stream_max_width = MAX_CAPTURE_WIDTH
        self._stream_max_height = MAX_CAPTURE_HEIGHT
        self._restart_count = 0
        self._last_restart: Optional[str] = None
        self._consecutive_failures = 0
        self._max_backoff = 8
        self._backend_used: Optional[str] = None

        # Per-camera single-threaded executor — ALL DShow operations
        # (open AND read) for this camera run on this one dedicated thread.
        # This satisfies COM STA requirements (object always used from its
        # creation thread) and eliminates cross-camera executor starvation.
        self._executor = ThreadPoolExecutor(
            max_workers=1,
            thread_name_prefix=f"dshow-{label}"
        )

    @property
    def camera_index(self) -> int:
        return self._camera_index

    @camera_index.setter
    def camera_index(self, value: int):
        self._camera_index = value

    @property
    def status(self) -> str:
        return self._status

    @property
    def error(self) -> Optional[str]:
        return self._error

    # â”€â”€ lifecycle â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    def start(self):
        self._running = True
        self._thread = threading.Thread(target=self._run, daemon=True,
                                        name=f"{self.label}-thread")
        self._thread.start()

    def stop(self, timeout=5):
        self._running = False
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout)
        self._thread = None
        # pop index + shutdown executor to avoid STA thread leak
        with _ACTIVE_LOCK:
            if _ACTIVE_INDICES.get(self._camera_index) == self.label:
                _ACTIVE_INDICES.pop(self._camera_index, None)
        try:
            self._executor.shutdown(wait=False, cancel_futures=True)
        except Exception:
            pass
        # recreate executor for next start()
        self._executor = ThreadPoolExecutor(max_workers=1, thread_name_prefix=f"dshow-{self.label}")

    def force_release(self):
        """Aggressively release the camera handle for browser access."""
        if self._cap:
            for attempt in range(3):
                try:
                    self._cap.release()
                    logger.info("[%s] Force release succeeded on attempt %d", self.label, attempt + 1)
                    break
                except Exception:
                    pass
                time.sleep(0.3)
            self._cap = None
        with _ACTIVE_LOCK:
            if _ACTIVE_INDICES.get(self._camera_index) == self.label:
                _ACTIVE_INDICES.pop(self._camera_index, None)

    # â”€â”€ frame access â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    def get_latest_frame(self):
        with self._lock:
            return self._latest_frame

    def get_latest_jpeg(self) -> Optional[bytes]:
        """Return the pre-encoded JPEG for streaming (zero encoding overhead)."""
        with self._lock:
            return self._latest_jpeg

    def get_latest_jpeg_seq(self) -> tuple[int, Optional[bytes]]:
        """Return the latest (frame_seq, jpeg)."""
        with self._lock:
            return self._frame_seq, self._latest_jpeg

    def wait_for_frame(self, last_seq: int, timeout: float = 0.5) -> tuple[int, Optional[bytes]]:
        """Block until a frame newer than last_seq is available, or timeout."""
        with self._frame_cond:
            if self._frame_seq <= last_seq:
                self._frame_cond.wait(timeout)
            return self._frame_seq, self._latest_jpeg

    @property
    def frame_age(self) -> float:
        """Seconds since the last successful frame capture (monotonic)."""
        ts = self._latest_frame_ts
        if ts is None:
            return float("inf")
        return time.monotonic() - ts

    def capture_snapshot(self) -> Optional[bytes]:
        """Capture a single JPEG-encoded snapshot at current adaptive quality."""
        with self._lock:
            if self._latest_jpeg is not None:
                return self._latest_jpeg
            if self._latest_frame is not None and self._cv2 is not None:
                ok, buf = self._cv2.imencode(
                    ".jpg", self._latest_frame,
                    [self._cv2.IMWRITE_JPEG_QUALITY, self.stream_quality])
                if ok:
                    return buf.tobytes()
        return None

    # â”€â”€ camera probing â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    def _probe_capabilities(self, cap, backend_name: str):
        caps = self.capabilities
        caps.detected = True
        try:
            caps.backend = getattr(cap, "getBackendName", lambda: backend_name)() \
                           if hasattr(cap, "getBackendName") else backend_name
        except Exception:
            caps.backend = backend_name
        caps.width  = int(cap.get(self._cv2.CAP_PROP_FRAME_WIDTH))
        caps.height = int(cap.get(self._cv2.CAP_PROP_FRAME_HEIGHT))
        raw_fps = cap.get(self._cv2.CAP_PROP_FPS)
        caps.fps = raw_fps if raw_fps and raw_fps > 0 else 30.0
        logger.info("[%s] Detected: %dx%d @ %.1ffps (backend: %s)",
                    self.label, caps.width, caps.height, caps.fps, caps.backend)

    def _select_tier(self):
        """Pick the best quality tier the camera can handle."""
        cv2 = self._cv2
        caps = self.capabilities
        ordered = ["1080p", "720p", "480p"]
        for label in ordered:
            t = QUALITY_TIERS[label]
            if t["width"] <= self._stream_max_width and \
               t["height"] <= self._stream_max_height and \
               t["width"] <= max(caps.width, 1280):
                logger.info("[%s] Selected tier: %s", self.label, label)
                return label, t
        fallback = "480p"
        logger.info("[%s] Fallback tier: %s", self.label, fallback)
        return fallback, QUALITY_TIERS[fallback]

    def _apply_settings(self, cap, tier: dict, backend_name: str):
        """
        Apply resolution and FPS to the capture device.
        For DShow: set resolution directly (works reliably).
        For MSMF:  skip resolution change (causes stride errors on 4K cams)
                   and let _run() handle software resize.
        """
        cv2 = self._cv2

        target_w = tier["width"]
        target_h = tier["height"]
        self._target_width = target_w
        self._target_height = target_h
        self.stream_quality = tier.get("jpeg_quality", self.stream_quality)

        if backend_name == "MSMF":
            # DO NOT set resolution via MSMF â€” it causes stride mismatch.
            # We'll read native frames and resize in software.
            actual_w = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
            actual_h = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
            actual_fps = cap.get(cv2.CAP_PROP_FPS) or 30.0
            logger.info("[%s] MSMF: keeping native %dx%d, will resize in software to %dx%d",
                        self.label, actual_w, actual_h, target_w, target_h)
        else:
            # DShow / default: set resolution directly
            # Try MJPG first for better throughput
            try:
                fourcc = cv2.VideoWriter_fourcc(*"MJPG")
                cap.set(cv2.CAP_PROP_FOURCC, fourcc)
                cap.grab()  # flush
                actual_fourcc = int(cap.get(cv2.CAP_PROP_FOURCC))
                if actual_fourcc != fourcc:
                    logger.info("[%s] MJPG rejected by driver â€” keeping default", self.label)
            except Exception:
                pass

            cap.set(cv2.CAP_PROP_FRAME_WIDTH, target_w)
            cap.set(cv2.CAP_PROP_FRAME_HEIGHT, target_h)
            cap.set(cv2.CAP_PROP_FPS, self._target_fps)
            cap.grab()  # flush

            actual_w = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
            actual_h = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
            actual_fps = cap.get(cv2.CAP_PROP_FPS) or 30.0

        logger.info("[%s] Applied: %dx%d @ %.1ffps (target %d), JPEG quality=%d",
                    self.label, actual_w, actual_h, actual_fps,
                    self._target_fps, self.stream_quality)

        self.active_tier = {
            "label": tier.get("label", "custom"),
            "width":  target_w,
            "height": target_h,
            "preview_fps": self._target_fps,
            "jpeg_quality": self.stream_quality,
            "actual_width":  actual_w,
            "actual_height": actual_h,
            "actual_fps":    round(actual_fps, 1),
        }
        return actual_w, actual_h

    # â”€â”€ hot reconfigure â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    def reconfigure(self, width=None, height=None, jpeg_quality=None, fps=None):
        """Hot-reconfigure camera without full restart."""
        if not self._running:
            return {"status": "error", "message": "Camera not running"}
        if width:
            self._target_width = max(width, 320)
            self._stream_max_width = max(width, 320)
        if height:
            self._target_height = max(height, 240)
            self._stream_max_height = max(height, 240)
        if jpeg_quality:
            self.stream_quality = min(max(jpeg_quality, 10), 100)
        if fps:
            self._target_fps = min(max(fps, 1), 60)
        self.active_tier.update({
            "width": self._target_width,
            "height": self._target_height,
            "jpeg_quality": self.stream_quality,
            "preview_fps": self._target_fps,
        })
        return {
            "status": "ok",
            "width":  self._target_width,
            "height": self._target_height,
            "fps":    self._target_fps,
            "jpeg_quality": self.stream_quality,
        }

    # â”€â”€ info â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    def get_info(self) -> dict:
        """Return full camera info for admin panel."""
        return {
            "name":  self.label,
            "index": self._camera_index,
            "status": self._status,
            "error":  self._error,
            "capabilities": self.capabilities.to_dict(),
            "active_tier":  self.active_tier,
            "jpeg_quality": self.stream_quality,
            "capture_fps":  self.capture_fps,
            "watchdog": {
                "restart_count": self._restart_count,
                "last_restart":  self._last_restart,
                "consecutive_failures": self._consecutive_failures,
            },
        }

    # â”€â”€ capture loop â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    def _run(self):
        try:
            import cv2
            self._cv2 = cv2
        except ImportError:
            self._status = "degraded"
            self._error = f"OpenCV unavailable: {self.label}"
            return

        while self._running:
            # Guard: prevent two streams from using the same device
            with _ACTIVE_LOCK:
                holder = _ACTIVE_INDICES.get(self._camera_index)
                if holder and holder != self.label:
                    self._status = "degraded"
                    self._error = f"Camera {self._camera_index} already used by {holder}"
                    logger.warning("[%s] Index %d locked by %s, backing off",
                                   self.label, self._camera_index, holder)
                    self._wait(5)
                    continue
                _ACTIVE_INDICES[self._camera_index] = self.label

            # Open camera â€” serialized through the DShow executor
            try:
                # Open via this camera's own dedicated DShow thread.
                # Both open AND read use self._executor so all DShow calls
                # for this device happen on one stable STA thread.
                future = self._executor.submit(_open_camera_multibackend, self._camera_index)
                cap, backend_name = future.result(timeout=15)
            except Exception:
                cap, backend_name = None, None

            if cap is None:
                with _ACTIVE_LOCK:
                    _ACTIVE_INDICES.pop(self._camera_index, None)
                self._status = "degraded"
                self._error = f"Camera {self._camera_index} unavailable"
                # Clear stale cached frame so snapshot endpoints return 204
                # (no content) instead of a frozen old JPEG that makes the
                # UI think the camera is alive when it is not.
                with self._lock:
                    self._latest_jpeg = None
                    self._latest_frame = None
                backoff = min(2 ** self._consecutive_failures, self._max_backoff)
                self._consecutive_failures += 1
                logger.warning("[%s] Cannot open camera %d, retry in %ds",
                               self.label, self._camera_index, backoff)
                self._wait(backoff)
                continue

            self._cap = cap
            self._backend_used = backend_name
            self._status = "running"
            self._error = None
            self._consecutive_failures = 0

            # Set buffer size to 1 to reduce latency
            try:
                cap.set(cv2.CAP_PROP_BUFFERSIZE, 1)
            except Exception:
                pass

            # Probe + configure
            self._probe_capabilities(cap, backend_name)
            tier_label, tier = self._select_tier()
            tier["label"] = tier_label
            native_w, native_h = self._apply_settings(cap, tier, backend_name)

            # Determine if we need software resize
            needs_sw_resize = (native_w != self._target_width or
                               native_h != self._target_height)

            fail_streak = 0
            target_interval = 1.0 / self._target_fps if self._target_fps > 0 else 0.05

            while self._running:
                t0 = time.monotonic()
                try:
                    # Read on the same camera-dedicated thread as the open.
                    future = self._executor.submit(cap.read)
                    ret, frame = future.result(timeout=2)
                except Exception as e:
                    fail_streak += 1
                    if fail_streak <= 3:
                        logger.warning("[%s] Frame read error (skipping): %s", self.label, e)
                    if fail_streak >= 30:
                        logger.warning("[%s] %d consecutive read failures, reconnecting",
                                       self.label, fail_streak)
                        break
                    time.sleep(0.01)
                    continue

                if not ret or frame is None:
                    fail_streak += 1
                    if fail_streak >= 30:
                        logger.warning("[%s] %d consecutive read failures, reconnecting",
                                       self.label, fail_streak)
                        break
                    time.sleep(0.01)
                    continue

                fail_streak = 0

                # Software resize if needed
                if needs_sw_resize and frame.shape[1] != self._target_width:
                    frame = cv2.resize(frame, (self._target_width, self._target_height),
                                       interpolation=cv2.INTER_AREA)

                # Encode JPEG for streaming
                stream_jpeg = self._encode_stream_jpeg(frame)

                # Publish
                with self._frame_cond:
                    self._latest_frame = frame
                    self._latest_jpeg = stream_jpeg
                    self._latest_frame_ts = time.monotonic()
                    self._frame_seq += 1
                    self._frame_cond.notify_all()

                # Pace to target FPS
                elapsed = time.monotonic() - t0
                remaining = target_interval - elapsed
                if remaining > 0.001:
                    time.sleep(remaining)

            # Cleanup before reconnect
            try:
                cap.release()
            except Exception:
                pass
            self._cap = None
            with _ACTIVE_LOCK:
                _ACTIVE_INDICES.pop(self._camera_index, None)

            if self._running:
                import random, zoneinfo
                self._consecutive_failures = min(self._consecutive_failures + 1, 6)
                backoff = min(2 ** self._consecutive_failures, self._max_backoff) + random.uniform(0, 0.5)
                self._restart_count += 1
                _pht = zoneinfo.ZoneInfo("Asia/Manila")
                self._last_restart = __import__("datetime").datetime.now(_pht).isoformat()
                self._status = "reconnecting"
                logger.info("[%s] Watchdog restart #%d, backoff %.1fs",
                            self.label, self._restart_count, backoff)
                self._wait(backoff)

    def _encode_stream_jpeg(self, frame) -> Optional[bytes]:
        """Downscale frame to stream resolution and encode as JPEG."""
        cv2 = self._cv2
        if frame is None or cv2 is None:
            return None
        try:
            # If frame is larger than target, resize
            h, w = frame.shape[:2]
            if w > self._target_width or h > self._target_height:
                scale = min(self._target_width / w, self._target_height / h)
                new_w, new_h = int(w * scale), int(h * scale)
                frame = cv2.resize(frame, (new_w, new_h), interpolation=cv2.INTER_AREA)
            ok, buf = cv2.imencode(".jpg", frame,
                                   [cv2.IMWRITE_JPEG_QUALITY, self.stream_quality])
            if ok:
                return buf.tobytes()
        except Exception:
            pass
        return None

    def _wait(self, seconds: float):
        """Sleep in small increments so stop() is responsive."""
        end = time.monotonic() + seconds
        while self._running and time.monotonic() < end:
            time.sleep(0.2)


# â”€â”€ detect_cameras â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
def detect_cameras(max_check: int = 5) -> list[dict]:
    """
    Scan system for available camera devices and probe their capabilities.
    """
    try:
        import cv2
    except ImportError:
        return []

    cameras = []
    for idx in range(max_check):
        cap, opened_via = _open_camera_multibackend(idx)
        if cap is None:
            continue
        working = True
        try:
            # Set DShow to highest resolution before probing, so cameras
            # report their true max capability (not the default 640x480).
            if opened_via == "DSHOW":
                cap.set(cv2.CAP_PROP_FRAME_WIDTH, 3840)
                cap.set(cv2.CAP_PROP_FRAME_HEIGHT, 2160)
            w = int(cap.get(cv2.CAP_PROP_FRAME_WIDTH))
            h = int(cap.get(cv2.CAP_PROP_FRAME_HEIGHT))
            fps = cap.get(cv2.CAP_PROP_FPS) or 0
            backend = opened_via or "unknown"
            logger.info("Detected camera %d: %dx%d @ %.1ffps (backend=%s, opened_via=%s)",
                        idx, w, h, fps, backend, opened_via)
        except Exception:
            w = h = 0
            fps = 0
            backend = ""
            working = False
        try:
            cap.release()
        except Exception:
            pass
        # DShow needs a moment to fully release the device handle
        time.sleep(0.5)
        cameras.append({
            "index": idx,
            "width": w,
            "height": h,
            "fps": round(fps, 1),
            "backend": backend,
            "opened_via": opened_via,
            "working": working,
            "detected": True,
        })

    # â”€â”€ Respect config camera assignment â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    # The compiled access_control._auto_detect_cameras picks cameras by
    # descending resolution, and when quality is equal it uses list order.
    # Sort so that the config-specified cam1 device appears first in the
    # returned list â€” this ensures auto-detect assigns it to cam1 even
    # when both cameras report the same resolution (two identical EMEET 4K).
    try:
        import os as _os
        _cam1_idx = int(_os.environ.get("CAM1_INDEX", "1"))
        _cam2_idx = int(_os.environ.get("CAM2_INDEX", "0"))
        _cam3_idx = int(_os.environ.get("CAM3_INDEX", "2"))
        def _sort_key(c):
            if c["index"] == _cam1_idx:
                return 0   # cam1 device → first
            if c["index"] == _cam2_idx:
                return 1   # cam2 device → second
            if c["index"] == _cam3_idx:
                return 2   # cam3 device → third
            return 3
        cameras.sort(key=_sort_key)
        logger.info("detect_cameras: sorted %d camera(s) by config order (cam1=%d, cam2=%d, cam3=%d)",
                    len(cameras), _cam1_idx, _cam2_idx, _cam3_idx)
    except Exception as e:
        logger.debug("detect_cameras sort skipped: %s", e)

    return cameras


