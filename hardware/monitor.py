import serial
import time
import sys

def monitor(port='COM8', baud=115200):
    try:
        ser = serial.Serial(port, baud, timeout=0.1)
        print(f"[SERIAL] Connected to {port} @ {baud} baud.", flush=True)
        print("[SERIAL] Make sure IO9/IO0 is DISCONNECTED from GND, then press EN/RST button on ESP32.", flush=True)
        
        last_ping = time.time()
        while True:
            # Periodic PING every 4 seconds
            if time.time() - last_ping > 4:
                try:
                    ser.write(b"PING\n")
                    ser.flush()
                except Exception:
                    pass
                last_ping = time.time()

            if ser.in_waiting > 0:
                raw = ser.readline()
                try:
                    line = raw.decode('utf-8', errors='replace').strip()
                    if line:
                        print(f"[ESP32] {line}", flush=True)
                except Exception as e:
                    print(f"[RAW] {raw}", flush=True)
            time.sleep(0.02)
    except KeyboardInterrupt:
        print("\n[SERIAL] Monitor stopped by user.", flush=True)
    except Exception as e:
        print(f"[SERIAL ERROR] {e}", flush=True)

if __name__ == '__main__':
    monitor()
