import requests
import json
import sys

API_BASE = "http://localhost:8080"

def test_rbac_and_branch_pricing():
    print("[1] Logging in as Franchise Owner (ceo@titan.fitness)...")
    res = requests.post(f"{API_BASE}/api/v1/owner/auth/login", json={
        "email": "ceo@titan.fitness",
        "password": "titan2026"
    })
    assert res.status_code == 200, f"Owner login failed: {res.status_code} {res.text}"
    login_data = res.json()
    token = login_data["token"]
    owner_email = login_data["owner_email"]
    print(f"    Owner Login OK! Token acquired for {owner_email}")

    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/json"
    }

    print("[2] Fetching owner gyms to find target branch...")
    res = requests.get(f"{API_BASE}/api/v1/owner/analytics", headers=headers)
    assert res.status_code == 200
    analytics = res.json()
    branches = analytics.get("branches", [])
    assert len(branches) > 0, "No branches found for owner"
    target_branch = branches[0]
    gym_id = target_branch["gym_id"]
    gym_name = target_branch["name"]
    print(f"    Target Branch: {gym_name} ({gym_id})")

    import time
    uname = f"cashier_{int(time.time())}"
    staff_payload = {
        "full_name": "Maria Santos",
        "username": uname,
        "pin_code": "5678",
        "role": "staff",
        "gym_id": gym_id,
        "gym_name": gym_name
    }
    res = requests.post(f"{API_BASE}/api/v1/owner/staff", headers=headers, json=staff_payload)
    assert res.status_code in (200, 201), f"Failed to create staff: {res.status_code} {res.text}"
    data = res.json()
    staff = data.get("staff", data)
    staff_id = staff["id"]
    print(f"    Created Staff ID: {staff_id}, Full Name: {staff['full_name']}, Role: {staff['role']}")

    print("[4] Listing staff accounts via owner portal endpoint...")
    res = requests.get(f"{API_BASE}/api/v1/owner/staff", headers=headers)
    assert res.status_code == 200
    staff_list = res.json().get("staff", [])
    found = any(s["id"] == staff_id and s["username"] == uname for s in staff_list)
    assert found, "Created staff not found in list"
    print(f"    Verified staff account in owner list (Total staff: {len(staff_list)})")

    print("[5] Setting branch-specific product price override...")
    # Fetch existing catalog
    res = requests.get(f"{API_BASE}/api/v1/owner/catalog", headers=headers)
    assert res.status_code == 200
    products = res.json().get("products", [])
    assert len(products) > 0, "No products in catalog"
    sample_prod = products[0]
    override_price = 285.50
    print(f"    Base product '{sample_prod['name']}' base price: {sample_prod['price']}")

    override_payload = {
        "product_id": sample_prod["id"],
        "gym_id": gym_id,
        "price": override_price,
        "stock": 99
    }
    res = requests.post(f"{API_BASE}/api/v1/owner/catalog/override", headers=headers, json=override_payload)
    assert res.status_code == 200, f"Failed to save branch override: {res.status_code} {res.text}"
    print(f"    Set Branch '{gym_name}' override price to {override_price}")

    print("[6] Simulating Desktop POS Heartbeat Sync...")
    license_token = target_branch.get("license_token") or target_branch.get("license_key") or ""
    sync_headers = {
        "Authorization": f"Bearer {license_token}",
        "Content-Type": "application/json"
    }
    sync_payload = {
        "gym_id": gym_id,
        "gym_name": gym_name,
        "owner_email": owner_email,
        "timestamp": "2026-09-03T12:00:00Z",
        "attendance_logs": [],
        "members": [],
        "face_vectors": [],
        "sales": []
    }
    res = requests.post(f"{API_BASE}/api/v1/sync/push", headers=sync_headers, json=sync_payload)
    assert res.status_code == 200, f"Sync failed: {res.status_code} {res.text}"
    sync_resp = res.json()

    # Verify staff accounts synced down to POS
    synced_staff = sync_resp.get("staff_accounts", [])
    synced_maria = next((s for s in synced_staff if s["id"] == staff_id), None)
    assert synced_maria is not None, "Staff account was not synced down in SyncResponse"
    print(f"    Sync verified: Desktop POS received staff account '{synced_maria['full_name']}' with PIN hash {synced_maria['pin_hash'][:8]}...")

    # Verify branch price override applied in catalog
    synced_catalog = sync_resp.get("remote_catalog", [])
    synced_prod = next((p for p in synced_catalog if p["id"] == sample_prod["id"]), None)
    assert synced_prod is not None, "Product was not found in synced remote_catalog"
    assert abs(synced_prod["price"] - override_price) < 0.01, f"Expected override price {override_price}, got {synced_prod['price']}"
    print(f"    Sync verified: Branch catalog price is successfully overridden to {synced_prod['price']}!")

    print("[7] Updating Staff PIN and Status...")
    res = requests.put(f"{API_BASE}/api/v1/owner/staff/{staff_id}", headers=headers, json={
        "pin_code": "9999",
        "is_active": True
    })
    assert res.status_code == 200
    print("    Staff PIN updated to '9999' successfully")

    print("[8] Deleting Test Staff...")
    res = requests.delete(f"{API_BASE}/api/v1/owner/staff/{staff_id}", headers=headers)
    assert res.status_code == 200
    print("    Staff deleted cleanly")

    print("\nALL RBAC AND BRANCH PRICING INTEGRATION TESTS PASSED 100%!")

if __name__ == "__main__":
    try:
        test_rbac_and_branch_pricing()
    except Exception as e:
        print(f"TEST FAILED: {e}")
        sys.exit(1)
