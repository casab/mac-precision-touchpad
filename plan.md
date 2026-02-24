# Fix Plan: Review Issues from Phases 1–9

## Phase 1: BT Transport Correctness (P0)

**Goal:** Fix the two bugs most likely to cause real hardware failures.

### 1a. Fix `HidDeviceAttributes` struct to match Windows SDK (32 bytes)

**File:** `crates/amt-ptp-bt/src/transport.rs` lines 77-93

The local struct is 10 bytes but the Windows `HID_DEVICE_ATTRIBUTES` is 32 bytes (includes `Reserved[11]`). The lower driver may validate the `Size` field.

**Changes:**
- Add `reserved: [u16; 11]` to the struct (after `version_number`)
- The `size` field assignment on line 86 will automatically pick up the new 32-byte size via `size_of`
- The `WDF_MEMORY_DESCRIPTOR_INIT_BUFFER` call on line 93 will also auto-correct

### 1b. Fix `WdfTimerStop(FALSE)` race in suspend path

**File:** `crates/amt-ptp-bt/src/recovery.rs` line 123

`WdfTimerStop` with `FALSE` (don't wait) means the timer callback could still be running when `WdfIoTargetStop` cancels I/O. Since `evt_self_managed_io_suspend` runs at PASSIVE_LEVEL, we can safely wait.

**Changes:**
- Change `FALSE as BOOLEAN` to `TRUE as BOOLEAN` on line 123
- Update the comment from "Don't wait" to "Wait for timer callback to complete"

### 1c. Check `WdfIoTargetStart` return value

**File:** `crates/amt-ptp-bt/src/self_managed_io.rs` lines 180-185

The return value of `WdfIoTargetStart` is silently discarded. If it fails, subsequent operations will fail with confusing errors.

**Changes:**
- Capture the NTSTATUS return value
- Log and return the error if `!NT_SUCCESS`

---

## Phase 2: Dead Code Removal

**Goal:** Remove unused code that was either never wired up or replaced during development.

### 2a. Remove dead `SetFeaturePacket` struct

**File:** `crates/amt-ptp-bt/src/transport.rs` lines 140-145

This struct is defined inside `activate_multitouch` but never used — the actual code uses `HID_XFER_PACKET` from `wdk_sys`.

**Changes:**
- Delete lines 140-145 (the struct definition)

### 2b. Remove dead `recovery_work_item`

The work item is created but `WdfWorkItemEnqueue` is never called anywhere. The recovery timer handles all retry logic.

**Files:**
- `crates/amt-ptp-bt/src/recovery.rs`:
  - Delete `create_recovery_objects` lines 62-83 (work item creation)
  - Delete `evt_recovery_work_item` function (lines 198-221)
- `crates/amt-ptp-bt/src/device.rs`:
  - Remove `recovery_work_item: WDFWORKITEM` field (line 78)
  - Remove `self.recovery_work_item = core::ptr::null_mut()` from `init_defaults` (line 111)

---

## Phase 3: Concurrency Safety (`&mut` aliasing)

**Goal:** Eliminate undefined behavior from concurrent `&mut` references to `DeviceContext`.

**Problem:** Both drivers obtain `&mut *get_device_context(device)` from multiple WDF callbacks that can run concurrently on different CPUs:
- USB: continuous reader callback (DISPATCH_LEVEL) vs IOCTL dispatch (PASSIVE_LEVEL)
- BT: VHF callbacks vs BT read completion callback vs recovery timer

**Affected fields** (written by one callback, read by another):
- `ptp_input_on`, `ptp_report_touch`, `ptp_report_button` — set by feature IOCTL/VHF, read by input callback
- `vhf_ready` (BT only) — set by VHF ready callback, read/written by input callback
- `device_configured` (BT only) — set by suspend/init, read by input callback

**Approach:** Change from `&mut DeviceContext` to raw pointer field access. Instead of:
```rust
let ctx = unsafe { &mut *get_device_context(device) };
ctx.ptp_input_on = true;
```
Use:
```rust
let ctx = unsafe { get_device_context(device) };
unsafe { (*ctx).ptp_input_on = true };
```

This avoids creating `&mut` references that violate Rust's aliasing rules when two callbacks run concurrently. The raw pointer access pattern is the correct approach for WDF device contexts (shared mutable kernel state).

Additionally, wrap the concurrently-accessed boolean fields in `core::sync::atomic::AtomicBool` to ensure proper visibility across CPUs:

**USB DeviceContext changes** (`crates/amt-ptp-usb/src/device.rs`):
- `ptp_input_on: bool` → `ptp_input_on: AtomicBool`
- `ptp_report_touch: bool` → `ptp_report_touch: AtomicBool`
- `ptp_report_button: bool` → `ptp_report_button: AtomicBool`

**BT DeviceContext changes** (`crates/amt-ptp-bt/src/device.rs`):
- `ptp_input_on: bool` → `ptp_input_on: AtomicBool`
- `ptp_report_touch: bool` → `ptp_report_touch: AtomicBool`
- `ptp_report_button: bool` → `ptp_report_button: AtomicBool`
- `vhf_ready: bool` → `vhf_ready: AtomicBool`
- `device_configured: bool` → `device_configured: AtomicBool`

**All callers** must be updated:
- Reads: `ctx.ptp_input_on` → `ctx.ptp_input_on.load(Ordering::Relaxed)`
- Writes: `ctx.ptp_input_on = true` → `ctx.ptp_input_on.store(true, Ordering::Relaxed)`
- Init: `self.ptp_input_on = false` → `self.ptp_input_on = AtomicBool::new(false)`

`Relaxed` ordering is sufficient because:
- These are independent flag variables, not used to synchronize other memory
- WDF's own synchronization (I/O target stop, timer stop) provides the necessary barriers for lifecycle transitions

**Files to update (USB):**
- `crates/amt-ptp-usb/src/device.rs` — struct + init
- `crates/amt-ptp-usb/src/input.rs` — reads ptp_input_on, ptp_report_touch, ptp_report_button
- `crates/amt-ptp-usb/src/queue.rs` — reads/writes via hid module
- `crates/amt-ptp-usb/src/hid.rs` — writes ptp_input_on, ptp_report_touch, ptp_report_button

**Files to update (BT):**
- `crates/amt-ptp-bt/src/device.rs` — struct + init
- `crates/amt-ptp-bt/src/input.rs` — reads all flags, writes vhf_ready
- `crates/amt-ptp-bt/src/hid.rs` — writes ptp_input_on/touch/button, writes vhf_ready
- `crates/amt-ptp-bt/src/self_managed_io.rs` — writes device_configured, vhf_ready
- `crates/amt-ptp-bt/src/recovery.rs` — reads/writes device_configured
- `crates/amt-ptp-bt/src/transport.rs` — reads device_configured

---

## Phase 4: WDF Compliance

**Goal:** Pass WDF Verifier without warnings.

### 4a. Add `EvtIoStop` no-op to USB driver default queue

**File:** `crates/amt-ptp-usb/src/queue.rs`

WDF Verifier warns if a power-managed queue with pending requests has no `EvtIoStop` callback. The default parallel queue holds requests during IOCTL dispatch, and the manual input queue holds read requests across power transitions (but is non-power-managed, so it's exempt).

**Changes:**
- Add `evt_io_stop` callback function (acknowledge and leave pending, since filter drivers forward requests)
- Set `queue_config.EvtIoStop = Some(evt_io_stop)` on the default queue (line 54)

### 4b. Investigate `WdfPdoInitAllowForwardingRequestToParent`

The C USB driver calls this, but both Rust drivers are filter drivers (`WdfFdoInitSetFilter`), not PDO creators. This API requires a WDFPDOINIT, not a WDFDEVICEINIT from a filter. **Skip this fix** — the C driver call is likely a no-op or benign on filter device init, and adding it to the Rust driver would be incorrect API usage.

---

## Phase 5: Dependency Alignment

**Goal:** Eliminate version skew risk in WDK crates.

**File:** `Cargo.toml` (workspace root)

Currently: `wdk`/`wdk-alloc`/`wdk-panic` = 0.4.1, `wdk-sys`/`wdk-build` = 0.5.1

Pre-1.0 semver means 0.4→0.5 is a breaking change. The current mix works by accident but could break with any update.

**Changes:**
- Attempt to upgrade all WDK crates to 0.5.x
- If `wdk` 0.5.x doesn't exist yet, pin `wdk-sys` and `wdk-build` to 0.4.1 to match
- Run `cargo check` (or equivalent) to verify compatibility
- Document the chosen version in a comment

**Note:** This requires checking crates.io for available versions. If no compatible set exists, document the mismatch and move on.

---

## Phase 6: Static Assert for `Option<&T>` in `repr(C)`

**Goal:** Prevent silent layout breakage if Rust ever changes nullable reference optimization.

**Files:**
- `crates/amt-ptp-usb/src/device.rs`
- `crates/amt-ptp-bt/src/device.rs`

**Changes:**
Add a compile-time assertion near each DeviceContext:
```rust
const _: () = assert!(
    core::mem::size_of::<Option<&'static DeviceConfig>>() == core::mem::size_of::<*const DeviceConfig>(),
    "Option<&T> must be pointer-sized for repr(C) compatibility"
);
```

---

## Commit Strategy

Each phase gets its own commit:
1. `fix(bt): correct HidDeviceAttributes size and timer stop race`
2. `refactor(bt): remove dead SetFeaturePacket and unused work item`
3. `fix(usb,bt): use AtomicBool for concurrently-accessed DeviceContext flags`
4. `fix(usb): add EvtIoStop no-op for WDF Verifier compliance`
5. `chore: align WDK crate versions`
6. `fix(usb,bt): add static assert for Option<&T> pointer size in repr(C)`
