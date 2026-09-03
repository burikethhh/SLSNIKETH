import requests
import json
import time
import base64

BASE_URL = "http://localhost:8080"
ADMIN_KEY = "gympos_master_ceo_secret_2026"
ADMIN_HEADERS = {
    "Content-Type": "application/json",
    "Authorization": f"Bearer {ADMIN_KEY}"
}

def decode_token_payload(token):
    # token format: GPOS-<base64_json_claims>.<base64_signature>
    raw = token.replace("GPOS-", "")
    parts = raw.split(".")
    payload_b64 = parts[0]
    padded = payload_b64 + "=" * ((4 - len(payload_b64) % 4) % 4)
    decoded_bytes = base64.urlsafe_b64decode(padded)
    return json.loads(decoded_bytes.decode("utf-8"))

def test_full_license_distribution_flow():
    timestamp = int(time.time())
    owner_email = f"spartan_{timestamp}@spartanfit.com"
    owner_pass = "spartan2026"
    company_name = f"Spartan Fitness Group {timestamp}"

    print(f"\n=======================================================")
    print(f"TESTING CEO-ONLY LICENSE DISTRIBUTION & HIERARCHY FLOW")
    print(f"=======================================================\n")

    # Step 1: Owner Self-Registration
    print("[1] Registering prospective gym owner on Owner Portal...")
    reg_resp = requests.post(f"{BASE_URL}/api/v1/owner/auth/register", json={
        "company_name": company_name,
        "email": owner_email,
        "password": owner_pass
    })
    assert reg_resp.status_code == 201, f"Registration failed: {reg_resp.text}"
    reg_data = reg_resp.json()
    owner_token = reg_data["token"]
    print(f"    Owner Registered OK: {owner_email}, Token: {owner_token}")

    owner_headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {owner_token}"
    }

    # Step 2: Owner lists branches (should be empty initially)
    print("[2] Fetching initial branches from Owner Portal...")
    branches_resp = requests.get(f"{BASE_URL}/api/v1/owner/branches", headers=owner_headers)
    assert branches_resp.status_code == 200, f"Failed to get branches: {branches_resp.text}"
    branches_data = branches_resp.json()
    assert branches_data["count"] == 0, f"Expected 0 branches initially, got {branches_data['count']}"
    print("    Initial branches count: 0")

    # Step 3: Owner Requests Branch 1 (No Self-Signing)
    branch1_name = f"Spartan Gym - Downtown Branch {timestamp}"
    print(f"[3] Owner requests '{branch1_name}'...")
    req_branch_resp = requests.post(f"{BASE_URL}/api/v1/owner/gyms", headers=owner_headers, json={
        "name": branch1_name,
        "owner_email": owner_email,
        "tier": "pro",
        "duration_days": 30
    })
    assert req_branch_resp.status_code == 201, f"Failed to create branch: {req_branch_resp.text}"
    b1_data = req_branch_resp.json()
    branch1_id = b1_data["gym_id"]
    print(f"    Branch created! Gym ID: {branch1_id}")
    print(f"    Status: {b1_data.get('status')}")
    assert b1_data.get("status") == "pending_license", f"Status should be pending_license, got {b1_data.get('status')}"
    assert b1_data.get("license_key") is None, "Owner must NOT be able to self-issue license keys!"
    print("    VERIFIED: Owner cannot self-sign RSA license keys (license_key is None).")

    # Step 4: Verify branch is unlicensed in owner's portal
    branches_resp = requests.get(f"{BASE_URL}/api/v1/owner/branches", headers=owner_headers)
    branches = branches_resp.json()["branches"]
    assert len(branches) == 1
    assert branches[0]["license_key"] is None or branches[0]["license_key"] == ""
    print("    Owner Portal branch table shows: Awaiting CEO License Key.")

    # Step 5: CEO Master Command Center inspects Owner Hierarchy
    print("[5] CEO queries /api/v1/admin/owners hierarchy...")
    admin_hier_resp = requests.get(f"{BASE_URL}/api/v1/admin/owners", headers=ADMIN_HEADERS)
    assert admin_hier_resp.status_code == 200, f"Admin hierarchy failed: {admin_hier_resp.text}"
    hierarchy = admin_hier_resp.json()

    target_owner = next((o for o in hierarchy if o["owner_email"] == owner_email), None)
    assert target_owner is not None, f"Owner {owner_email} not found in CEO hierarchy!"
    print(f"    Found owner in CEO hierarchy: {target_owner['company_name']}")
    assert target_owner["total_branches"] == 1
    assert target_owner["pending_licenses_count"] == 1
    print(f"    CEO sees: {target_owner['total_branches']} Branch(es), {target_owner['pending_licenses_count']} Pending License(s).")

    target_branch1 = next((b for b in target_owner["branches"] if b["gym_id"] == branch1_id), None)
    assert target_branch1 is not None
    assert target_branch1["is_license_active"] == False
    print("    Target branch confirmed pending in CEO view.")

    # Step 6: CEO Issues RSA-2048 License Key specifically for Branch 1
    print(f"[6] CEO issues RSA-2048 license key for Branch 1 ({branch1_id})...")
    issue_resp = requests.post(f"{BASE_URL}/api/v1/admin/branches/{branch1_id}/issue-key", headers=ADMIN_HEADERS, json={
        "tier": "pro",
        "duration_days": 60
    })
    assert issue_resp.status_code == 200, f"CEO issue key failed: {issue_resp.text}"
    issue_data = issue_resp.json()
    b1_key = issue_data["license_key"]
    assert b1_key.startswith("GPOS-"), f"Invalid token format: {b1_key}"
    print(f"    Issued Key: {b1_key[:32]}... (Expires in 60 days)")

    # Decode and verify cryptographic claims for Branch 1
    b1_claims = decode_token_payload(b1_key)
    print(f"    Decoded Claims Gym ID: {b1_claims['gym_id']}")
    print(f"    Decoded Claims Gym Name: {b1_claims['gym_name']}")
    print(f"    Decoded Claims Owner Email: {b1_claims['owner_email']}")
    assert b1_claims["gym_id"] == branch1_id, "Key claims gym_id must match Branch 1!"
    assert b1_claims["owner_email"] == owner_email, "Key claims owner_email must match owner!"

    # Step 7: Verify Key appears in Owner Portal
    print("[7] Verifying issued key reflects in Owner Portal...")
    branches_resp = requests.get(f"{BASE_URL}/api/v1/owner/branches", headers=owner_headers)
    branches = branches_resp.json()["branches"]
    assert branches[0]["license_key"] == b1_key
    print("    VERIFIED: Owner Portal now displays CEO-issued license key ready for copy!")

    # Step 8: CEO directly adds Branch 2 for Owner with automatic key issuance
    branch2_name = f"Spartan Gym - Uptown Branch {timestamp}"
    print(f"[8] CEO directly provisions Branch 2 ('{branch2_name}') for owner...")
    admin_add_resp = requests.post(f"{BASE_URL}/api/v1/admin/owners/{owner_email}/branches", headers=ADMIN_HEADERS, json={
        "branch_name": branch2_name,
        "tier": "ultra",
        "duration_days": 90,
        "auto_issue_license": True
    })
    assert admin_add_resp.status_code == 201, f"Admin add branch failed: {admin_add_resp.text}"
    admin_b2 = admin_add_resp.json()
    branch2_id = admin_b2["gym_id"]
    b2_key = admin_b2["license_key"]
    print(f"    Branch 2 created by CEO! Gym ID: {branch2_id}")
    print(f"    Branch 2 Key: {b2_key[:32]}...")

    # Decode Branch 2 claims
    b2_claims = decode_token_payload(b2_key)
    assert b2_claims["gym_id"] == branch2_id, "Key claims gym_id must match Branch 2!"
    assert b2_claims["gym_id"] != branch1_id, "Branch 1 and Branch 2 MUST have distinct Gym IDs!"
    print("    VERIFIED: Branch 2 has its own unique, isolated cryptographic license key!")

    # Step 9: Verify CEO Hierarchy now shows 2 branches, 0 pending
    print("[9] Checking CEO Hierarchy update...")
    admin_hier_resp = requests.get(f"{BASE_URL}/api/v1/admin/owners", headers=ADMIN_HEADERS)
    hierarchy = admin_hier_resp.json()
    updated_owner = next((o for o in hierarchy if o["owner_email"] == owner_email), None)
    assert updated_owner["total_branches"] == 2
    assert updated_owner["active_licenses_count"] == 2
    assert updated_owner["pending_licenses_count"] == 0
    print(f"    CEO Hierarchy Updated: {updated_owner['total_branches']} Branches, {updated_owner['active_licenses_count']} Active Licenses, 0 Pending.")

    print("\n=======================================================")
    print("ALL CEO LICENSE DISTRIBUTION & HIERARCHY TESTS PASSED 100%!")
    print("=======================================================\n")

if __name__ == "__main__":
    test_full_license_distribution_flow()
