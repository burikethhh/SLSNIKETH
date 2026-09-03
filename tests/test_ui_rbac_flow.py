import os
import time
from playwright.sync_api import sync_playwright

ARTIFACT_DIR = r"C:\Users\USER\.gemini\antigravity-ide\brain\c7b8d742-a977-4945-8551-a4f2dcb10d4a"

def test_ui_flow():
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        context = browser.new_context(viewport={"width": 1400, "height": 900})
        page = context.new_page()

        print("[1] Navigating to Franchise Owner Portal...")
        page.goto("http://localhost:8080/portal", wait_until="networkidle")

        # Owner Auth Login
        print("[2] Logging into Owner Portal as ceo@titan.fitness...")
        page.fill("#auth-email-input", "ceo@titan.fitness")
        page.fill("#auth-pass-input", "titan2026")
        page.click("#modal-auth button[type='submit']")
        page.wait_for_selector("#modal-auth", state="hidden", timeout=10000)
        print("    Owner Portal login successful!")

        # Switch to Staff Tab
        print("[3] Switching to 'Staff & Cashiers' tab...")
        page.click("#tab-staff")
        page.wait_for_selector("#view-staff:not(.hidden)", timeout=5000)
        time.sleep(1)

        # Open Add Staff Modal
        print("[4] Opening Add Staff Modal...")
        page.click("button:has-text('+ Add Staff Member')")
        page.wait_for_selector("#modal-staff:not(.hidden)", timeout=5000)

        # Provision Cashier
        test_user = f"cashier_{int(time.time())}"
        print(f"[5] Creating Staff Credential for Elena Rostova ({test_user})...")
        page.fill("#staff-fullname-input", "Elena Rostova")
        page.fill("#staff-username-input", test_user)
        page.fill("#staff-pin-input", "7788")
        
        # Handle dialog alert
        page.on("dialog", lambda dialog: dialog.accept())
        page.click("#modal-staff button:has-text('Save Staff Credential')")
        time.sleep(2)

        # Screenshot Owner Portal Staff View
        staff_shot = os.path.join(ARTIFACT_DIR, "portal_staff_management.png")
        page.screenshot(path=staff_shot)
        print(f"    Saved Owner Portal screenshot: {staff_shot}")

        # Now test Desktop Terminal UI
        print("[6] Navigating to Desktop POS Webview...")
        terminal_url = f"file:///{os.path.abspath('desktop/webview/index.html').replace(os.sep, '/')}"
        page.goto(terminal_url, wait_until="networkidle")

        # Verify Lock Screen is displayed
        print("[7] Verifying Terminal PIN Lock Screen...")
        page.wait_for_selector("#terminal-lock-screen:not(.hidden)", timeout=5000)
        lock_shot = os.path.join(ARTIFACT_DIR, "terminal_pin_lock_screen.png")
        page.screenshot(path=lock_shot)
        print(f"    Saved Terminal Lock Screen screenshot: {lock_shot}")

        # Enter PIN 1234 (Default Cashier)
        print("[8] Entering Cashier PIN 1234 via numeric keypad...")
        page.click("button.pin-key:has-text('1')")
        page.click("button.pin-key:has-text('2')")
        page.click("button.pin-key:has-text('3')")
        page.click("button.pin-key:has-text('4')")
        time.sleep(1)

        # Verify Terminal Unlocked
        page.wait_for_selector("#terminal-lock-screen", state="hidden", timeout=5000)
        role_text = page.inner_text("#session-user-role")
        print(f"    Terminal Unlocked! Current Mode: {role_text}")
        assert "cashier" in role_text.lower() or "staff" in role_text.lower(), f"Unexpected role: {role_text}"

        cashier_shot = os.path.join(ARTIFACT_DIR, "terminal_unlocked_cashier.png")
        page.screenshot(path=cashier_shot)
        print(f"    Saved Cashier Terminal screenshot: {cashier_shot}")

        # Lock terminal again
        print("[9] Testing Shift Lock button...")
        page.click("#btn-lock-terminal")
        page.wait_for_selector("#terminal-lock-screen", state="visible", timeout=5000)
        print("    Terminal locked successfully!")

        # Test Master Owner Login
        print("[10] Testing Master Owner Login Modal...")
        page.click("button:has-text('Owner Master Login')")
        page.wait_for_selector("#modal-owner-login", state="visible", timeout=5000)
        page.fill("#owner-login-email", "ceo@titan.fitness")
        page.fill("#owner-login-pass", "titan2026")
        page.click("#modal-owner-login button[type='submit']")
        time.sleep(1)

        page.wait_for_selector("#terminal-lock-screen", state="hidden", timeout=5000)
        owner_role_text = page.inner_text("#session-user-role")
        print(f"    Terminal Unlocked! Master Mode: {owner_role_text}")
        assert "owner" in owner_role_text.lower() or "master" in owner_role_text.lower(), f"Unexpected role: {owner_role_text}"

        owner_shot = os.path.join(ARTIFACT_DIR, "terminal_unlocked_master_owner.png")
        page.screenshot(path=owner_shot)
        print(f"    Saved Master Owner Terminal screenshot: {owner_shot}")

        browser.close()
        print("\nALL UI FLOW TESTS COMPLETED WITH 100% SUCCESS!")

if __name__ == "__main__":
    test_ui_flow()
