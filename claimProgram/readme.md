# Token Claim Program: Function Documentation 

## Overview
The Token Claim Program enables secure and efficient distribution of tokens to beneficiaries. It supports features like bulk updates, blocking/unblocking users, and managing escrow wallets. This document details the purpose and functionality of each program instruction.

---

## Functions

### 1. `initialize`
**Purpose:**  
Sets up the data account and escrow wallet for managing token claims for a specific round, claim type, and batch. Transfers tokens from the sender’s wallet to the escrow wallet.

**Parameters:**  
- `round` - The round identifier.
- `claim_type` - Type of claim (e.g., IDO, SAFT).
- `batch` - Batch identifier.
- `_list_size` - Number of beneficiaries.
- `beneficiaries` - List of beneficiaries eligible for claiming.
- `amount` - Total token amount to be allocated.
- `decimals` - Token decimals for precision.

**Key Actions:**  
- Initializes the `data_account` with provided details.
- Checks if the `batch` already exists; returns an error if true.
- Transfers the specified amount of tokens from `wallet_to_withdraw_from` to `escrow_wallet`.

---

### 2. `release`
**Purpose:**  
Marks the tokens in the `data_account` as released or not released, enabling or disabling claims.

**Parameters:**  
- `_round`, `_claim_type`, `_batch` - Identifiers for the specific claim set.
- `released` - Boolean indicating whether tokens are released.

**Key Actions:**  
- Updates the `released` field of `data_account`.

---

### 3. `update_user_status`
**Purpose:**  
Updates the blocked status of a specific beneficiary in the `data_account`.

**Parameters:**  
- `_round`, `_claim_type`, `_batch` - Identifiers for the specific claim set.
- `user_wallet` - Public key of the user whose status is being updated.
- `blocked` - Boolean indicating whether the user is blocked.

**Key Actions:**  
- Finds the beneficiary by `user_wallet`.
- Updates the `is_blocked` field for the beneficiary.

**Validation:**  
- Returns an error if the beneficiary is not found.

---

### 4. `update_bulk_user_status`
**Purpose:**  
Blocks or unblocks all unclaimed beneficiaries in the `data_account`.

**Parameters:**  
- `_round`, `_claim_type`, `_batch` - Identifiers for the specific claim set.
- `blocked` - Boolean indicating whether users should be blocked.

**Key Actions:**  
- Iterates through `beneficiaries`.
- Updates `is_blocked` for unclaimed beneficiaries.

---

### 5. `withdraw_from_escrow`
**Purpose:**  
Withdraws tokens allocated to blocked beneficiaries from the escrow wallet to the admin’s wallet.

**Parameters:**  
- `_round`, `_claim_type`, `_batch` - Identifiers for the specific claim set.

**Key Actions:**  
- Calculates the sum of tokens allocated to blocked beneficiaries.
- Transfers the calculated amount from the `escrow_wallet` to the admin's associated token account (`admin_ata`).

**Validation:**  
- Utilizes a program-derived address (PDA) with seed-based validation for secure authority.

---

### 6. `claim`
**Purpose:**  
Allows a beneficiary to claim their allocated tokens from the escrow wallet.

**Parameters:**  
- `_round`, `_claim_type`, `_batch` - Identifiers for the specific claim set.

**Key Actions:**  
- Verifies the beneficiary's eligibility (not blocked, not claimed, not in process).
- Transfers the allocated tokens from the `escrow_wallet` to the beneficiary's associated token account (`beneficiary_ata`).
- Marks the beneficiary as claimed.

**Validation:**  
- Ensures the beneficiary is not blocked, has not already claimed, and is not mid-claim.

---

## Accounts and Structures

### `DataAccount`
Stores all metadata related to a specific round, claim type, and batch:
- `released` - Indicates if tokens are released for claiming.
- `round`, `claim_type`, `batch` - Identifiers.
- `token_amount` - Total token amount for the batch.
- `initializer`, `escrow_wallet`, `token_mint` - Public keys for related accounts.
- `beneficiaries` - List of beneficiaries with details such as:
  - `allocated_tokens`
  - `is_claimed`
  - `is_blocked`
  - `in_process`
- `decimals` - Token decimals.

---

## Error Codes
- **`InvalidSender`:** Occurs when the sender is not the initializer.
- **`ClaimNotAllowed`:** Occurs when claiming is restricted.
- **`BeneficiaryNotFound`:** Occurs if the beneficiary is missing.
- **`IsBatched`:** Indicates that the batch already exists.

---

## License
This program is open-source and available under the [MIT License](LICENSE).

