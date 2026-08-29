@echo off
title GymPOS
cd /d "%~dp0"

:: ============================================================
::  GymPOS Launcher
::  Standalone Edition â€” Solo Leveling Gym
:: ============================================================
::
::  Hardware configuration for this PC
::  â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
::  Cameras  : 3x EMEET SmartCam C60E 4K
::    cam1   : Face IN  / member recognition  (device index 1)
::    cam2   : Face OUT / member exit         (device index 0)
::    cam3   : Tailgate / overhead monitor    (device index 2)
::  ESP32    : Silicon Labs CP210x on COM3
::  RFID     : USB HID keyboard-emulation reader
::
::  To change camera assignments edit cam1_index / cam2_index
::  in STANDALONE\.env  (persists across restarts).
::
set CAM1_INDEX=1
set CAM2_INDEX=0
set CAM3_INDEX=2

:: â”€â”€ Kill any process still holding port 8000 â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
for /f "tokens=5" %%a in ('netstat -aon ^| findstr ":8000.*LISTENING" 2^>nul') do (
    taskkill /PID %%a /F >nul 2>&1
)

:: â”€â”€ Launch Electron shell (spawns Python backend) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
start "" "electron\node_modules\electron\dist\electron.exe" "electron"


