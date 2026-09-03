import os
import time
from playwright.sync_api import sync_playwright

BASE_URL = "http://localhost:8080"
ARTIFACT_DIR = r"C:\Users\USER\.gemini\antigravity-ide\brain\c7b8d742-a977-4945-8551-a4f2dcb10d4a"

def test_ui_ceo_hierarchy_flow():
    timestamp = int(time.time())
    owner_email = f"vanguard_{timestamp}@vanguard.fit"
    company_name = f"Vanguard Iron Gym {timestamp}"

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        context = browser.new_context(viewport={"width": 1440, "height": 960})
        page = context.new_page()

        print("\n[1] Navigating to Franchise Owner Portal (/portal.html)...")
        page.goto(f"{BASE_URL}/portal.html", wait_until="networkidle")
        time.sleep(1)

        print("[2] Testing Owner Registration Tab Switcher...")
        page.click("#tab-btn-auth-register")
        time.sleep(0.5)

        print(f"[3] Registering new client owner '{company_name}' ({owner_email})...")
        page.fill("#auth-reg-company", company_name)
        page.fill("#auth-reg-email", owner_email)
        page.fill("#auth-reg-pass", "vanguard2026")
        page.fill("#auth-reg-pass-confirm", "vanguard2026")

        # Handle browser alert popup automatically
        page.on("dialog", lambda dialog: dialog.accept())

        page.click("#form-auth-register button[type='submit']")
        time.sleep(1.5)

        # Verify portal modal hidden and dashboard active
        page.wait_for_selector("#modal-auth", state="hidden", timeout=6000)
        print("    Owner successfully registered and logged in!")

        shot1 = os.path.join(ARTIFACT_DIR, "portal_owner_registered.png")
        page.screenshot(path=shot1)
        print(f"    Saved Screenshot: {shot1}")

        print("[4] Switching to 'Multi-Branch Keys' Tab...")
        page.click("#tab-keys")
        time.sleep(1)

        print("[5] Requesting new branch location from owner portal...")
        page.click("#btn-open-create-branch")
        time.sleep(0.5)
        branch_name = f"Vanguard Iron - Makati Branch {timestamp}"
        page.fill("#owner-new-branch-name", branch_name)
        page.select_option("#owner-new-branch-tier", "pro")
        page.click("#btn-submit-create-branch")
        time.sleep(2)

        print("    Verifying branch shows 'Awaiting CEO License Key'...")
        page.wait_for_selector("span:has-text('Awaiting CEO License Key')", timeout=8000)
        shot2 = os.path.join(ARTIFACT_DIR, "portal_branch_awaiting_key.png")
        page.screenshot(path=shot2)
        print(f"    Saved Screenshot: {shot2}")

        print("[6] Opening CEO Master Command Center (/) in new page...")
        ceo_page = context.new_page()
        ceo_page.goto(f"{BASE_URL}/", wait_until="networkidle")
        time.sleep(1)

        print("[7] Verifying Collapsible Owner Hierarchy on CEO Dashboard...")
        # Authorize admin session if needed
        ceo_page.evaluate("""() => {
            localStorage.setItem('gympos_admin_key', 'gympos_master_ceo_secret_2026');
            fetchGyms();
        }""")
        time.sleep(1.5)

        # Expand our owner card if not expanded
        owner_card_selector = f"div:has-text('{owner_email}')"
        ceo_page.wait_for_selector(owner_card_selector, timeout=8000)
        print(f"    Found owner card for {owner_email} in CEO Hierarchy!")

        shot3 = os.path.join(ARTIFACT_DIR, "ceo_hierarchy_expanded_pending.png")
        ceo_page.screenshot(path=shot3)
        print(f"    Saved Screenshot: {shot3}")

        print("[8] Clicking 'Issue Key' for branch...")
        # Find the Issue Key button inside this owner card
        ceo_page.click(f"div:has-text('{owner_email}') button:has-text('Issue Key')")
        time.sleep(0.5)

        ceo_page.wait_for_selector("#modal-issue-branch-key", state="visible", timeout=5000)
        print("    Issue Key modal visible. Submitting 60-day RSA-2048 key issuance...")
        ceo_page.fill("#issue-key-duration", "60")
        ceo_page.click("button:has-text('Sign & Issue License')")
        time.sleep(1.5)

        # Verify License Output modal appears with GPOS token
        ceo_page.wait_for_selector("#license-output-modal", state="visible", timeout=6000)
        issued_token = ceo_page.input_value("#output-license-key")
        print(f"    CEO Signed Key: {issued_token[:30]}...")
        assert issued_token.startswith("GPOS-"), f"Unexpected key format: {issued_token}"

        shot4 = os.path.join(ARTIFACT_DIR, "ceo_license_issued_modal.png")
        ceo_page.screenshot(path=shot4)
        print(f"    Saved Screenshot: {shot4}")

        print("[9] Returning to Owner Portal and verifying active key...")
        page.bring_to_front()
        page.evaluate("() => loadOwnerDashboardData()")
        time.sleep(1.5)

        page.wait_for_selector("span:has-text('Active (Licensed)')", timeout=6000)
        page.wait_for_selector("button:has-text('Copy Key')", timeout=6000)
        print("    VERIFIED: Branch status updated to 'Active (Licensed)' with Copy Key button!")

        shot5 = os.path.join(ARTIFACT_DIR, "portal_branch_key_active.png")
        page.screenshot(path=shot5)
        print(f"    Saved Screenshot: {shot5}")

        print("\n=======================================================")
        print("ALL CEO HIERARCHY UI TESTS COMPLETED WITH 100% SUCCESS!")
        print("=======================================================\n")

        browser.close()

if __name__ == "__main__":
    test_ui_ceo_hierarchy_flow()
