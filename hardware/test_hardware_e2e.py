import serial
import time
import sys

def test_hardware(port='COM8', baud=115200):
    print("=" * 60)
    print("  GymPOS Hardware & Controller Diagnostic Suite")
    print(f"  Target Port: {port} @ {baud} baud")
    print("=" * 60)
    
    try:
        ser = serial.Serial(port, baud, timeout=1.5)
    except Exception as e:
        print(f"[FAIL] Could not open {port}: {e}")
        return False

    time.sleep(0.3)
    ser.reset_input_buffer()

    def send_and_expect(cmd, expected_prefix, timeout=2.0):
        print(f"\n>> Sending: {cmd}")
        ser.write((cmd + "\n").encode("utf-8"))
        ser.flush()
        start = time.time()
        matched = False
        received = []
        while time.time() - start < timeout:
            if ser.in_waiting:
                line = ser.readline().decode("utf-8", errors="replace").strip()
                if line:
                    received.append(line)
                    print(f"   << Received: {line}")
                    if expected_prefix in line:
                        matched = True
                        break
            time.sleep(0.02)
        if matched:
            print(f"   [PASS] Found expected response containing '{expected_prefix}'")
            return True
        else:
            print(f"   [WAIT/FAIL] Did not see '{expected_prefix}'. Got: {received}")
            return False

    results = {}

    # Test 1: PING
    results['PING'] = send_and_expect("PING", "ACK:PONG")

    # Test 2: STATUS
    results['STATUS'] = send_and_expect("STATUS", "ACK:STATUS:")

    # Test 3: BEEP
    results['BEEP'] = send_and_expect("BEEP", "ACK:BEEP")

    # Test 4: LCD Custom Display
    results['LCD'] = send_and_expect("LCD:GYMPOS DESKTOP|HARDWARE OK", "ACK:LCD")

    # Test 5: UNLOCK 2s + Relock
    results['UNLOCK'] = send_and_expect("UNLOCK:2", "ACK:UNLOCK")
    if results['UNLOCK']:
        print("   >> Waiting for auto-relock (ACK:RELOCK)...")
        start = time.time()
        relocked = False
        while time.time() - start < 4.0:
            if ser.in_waiting:
                line = ser.readline().decode("utf-8", errors="replace").strip()
                if line:
                    print(f"   << Received: {line}")
                    if "ACK:RELOCK" in line:
                        relocked = True
                        break
            time.sleep(0.05)
        results['AUTO_RELOCK'] = relocked
        if relocked:
            print("   [PASS] Auto-relock completed successfully!")
        else:
            print("   [WARN] Auto-relock ACK timed out.")

    # Test 6: ALERT_TAILGATE (Tailgate alarm)
    results['ALERT_TAILGATE'] = send_and_expect("ALERT_TAILGATE", "ACK:ALERT_TAILGATE")

    # Test 7: DENY reason text
    results['DENY'] = send_and_expect("DENY:EXPIRED", "ACK:DENY")

    # Test 8: Button Monitor (5 seconds)
    print("\n" + "-" * 60)
    print(">> Listening for Hardware Buttons (Entry=IO4, Exit=IO8) for 4s...")
    start = time.time()
    while time.time() - start < 4.0:
        if ser.in_waiting:
            line = ser.readline().decode("utf-8", errors="replace").strip()
            if line:
                print(f"   << [EVENT] {line}")
        time.sleep(0.05)

    ser.close()

    print("\n" + "=" * 60)
    print("  HARDWARE DIAGNOSTIC SUMMARY")
    print("=" * 60)
    for test, passed in results.items():
        status = "PASSED" if passed else "FAILED"
        print(f"  {test:<20}: [{status}]")
    print("=" * 60)
    return all(results.values())

if __name__ == '__main__':
    test_hardware()
