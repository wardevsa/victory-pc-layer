//! PlayStation 4 and 5 pads (DualShock 4, DualSense and DualSense Edge)
//! read as HID devices.
//!
//! Windows gives these pads no XInput slot, so without a helper program
//! they were not seen at all. When XInput has no pad in its first slot,
//! the first pad found here plays as that pad: over USB or Bluetooth, with
//! the face buttons where an Xbox pad has them (cross A, circle B, square X,
//! triangle Y), Options as Menu and Share, Create or a touchpad click as
//! View. Rumble reaches pads on USB.
//! shortcut: no adaptive triggers, light bar, touchpad or motion.

use super::{
    A, B, DOWN, LEFT, LEFT_SHOULDER, LEFT_STICK, MENU, PadState, RIGHT, RIGHT_SHOULDER,
    RIGHT_STICK, UP, VIEW, X, Y,
};

/// Sony's USB vendor id and the pads' product ids.
pub(super) const VENDOR: u16 = 0x054C;
pub(super) const PRODUCTS: [u16; 2] = [0x0CE6, 0x0DF2];
/// The DualShock 4's: the first model, the second, and Sony's USB wireless
/// adapter, which passes on a DualShock 4's reports.
pub(super) const PS4_PRODUCTS: [u16; 3] = [0x05C4, 0x09CC, 0x0BA0];

/// Which pad it is: their reports differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Model {
    /// DualSense and DualSense Edge.
    Ps5,
    /// DualShock 4.
    Ps4,
}

/// The model of a Sony pad with this product id, if it is one we read.
pub(super) fn model(vendor: u16, product: u16) -> Option<Model> {
    match (vendor, product) {
        (VENDOR, p) if PRODUCTS.contains(&p) => Some(Model::Ps5),
        (VENDOR, p) if PS4_PRODUCTS.contains(&p) => Some(Model::Ps4),
        _ => None,
    }
}

/// How a pad is connected: its reports differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Link {
    Usb,
    Bluetooth,
}

/// One input report as the pad sent it (report id first), or `None` if it
/// is not one of its input reports.
///
/// A PlayStation 5 pad on USB sends id 0x01 with sticks, triggers and
/// buttons from byte 1. Over Bluetooth it sends a short id 0x01 report
/// (sticks, buttons, then triggers) until it is asked for its full one, id
/// 0x31, which holds the USB layout from byte 2.
///
/// A DualShock 4 sends the short layout as id 0x01 on USB, and over
/// Bluetooth until it is asked for more; its full Bluetooth report, id
/// 0x11, holds the same from byte 3. Its button bits are the DualSense's.
pub(super) fn parse(model: Model, link: Link, report: &[u8]) -> Option<PadState> {
    // Sticks and triggers (lx, ly, rx, ry, l2, r2), and the three button
    // bytes.
    let (axes, buttons): ([u8; 6], [u8; 3]) = match (model, link, report) {
        (Model::Ps5, Link::Usb, [0x01, lx, ly, rx, ry, l2, r2, _, b0, b1, b2, ..]) => {
            ([*lx, *ly, *rx, *ry, *l2, *r2], [*b0, *b1, *b2])
        }
        (Model::Ps5, Link::Bluetooth, [0x31, _, lx, ly, rx, ry, l2, r2, _, b0, b1, b2, ..]) => {
            ([*lx, *ly, *rx, *ry, *l2, *r2], [*b0, *b1, *b2])
        }
        (Model::Ps5, Link::Bluetooth, [0x01, lx, ly, rx, ry, b0, b1, b2, l2, r2, ..])
        | (Model::Ps4, _, [0x01, lx, ly, rx, ry, b0, b1, b2, l2, r2, ..])
        | (Model::Ps4, Link::Bluetooth, [0x11, _, _, lx, ly, rx, ry, b0, b1, b2, l2, r2, ..]) => {
            ([*lx, *ly, *rx, *ry, *l2, *r2], [*b0, *b1, *b2])
        }
        _ => return None,
    };
    let [lx, ly, rx, ry, l2, r2] = axes;
    let [b0, b1, b2] = buttons;
    // Centred at 128; up is 0 on the pad, positive for the title.
    let across = |v: u8| ((f32::from(v) - 128.0) / 127.0).clamp(-1.0, 1.0);
    let up = |v: u8| ((128.0 - f32::from(v)) / 127.0).clamp(-1.0, 1.0);
    // The d-pad is a hat: 0 up, clockwise in eighths, 8 and above none.
    let hat = match b0 & 0x0F {
        0 => UP,
        1 => UP | RIGHT,
        2 => RIGHT,
        3 => DOWN | RIGHT,
        4 => DOWN,
        5 => DOWN | LEFT,
        6 => LEFT,
        7 => UP | LEFT,
        _ => 0,
    };
    // (button byte, bit, button). The PS and mute buttons have no Xbox pad
    // button and are left out, as are the triggers' own click bits (the
    // triggers are axes).
    let bits: [(usize, u8, u32); 11] = [
        (0, 0x10, X),
        (0, 0x20, A),
        (0, 0x40, B),
        (0, 0x80, Y),
        (1, 0x01, LEFT_SHOULDER),
        (1, 0x02, RIGHT_SHOULDER),
        (1, 0x10, VIEW),
        (1, 0x20, MENU),
        (1, 0x40, LEFT_STICK),
        (1, 0x80, RIGHT_STICK),
        (2, 0x02, VIEW),
    ];
    let bytes = [b0, b1, b2];
    let pressed = bits
        .iter()
        .filter(|&&(byte, mask, _)| bytes[byte] & mask != 0)
        .fold(hat, |all, &(_, _, bit)| all | bit);
    Some(PadState {
        buttons: pressed,
        left_trigger: f32::from(l2) / 255.0,
        right_trigger: f32::from(r2) / 255.0,
        left_x: across(lx),
        left_y: up(ly),
        right_x: across(rx),
        right_y: up(ry),
    })
}

/// The output report that sets the two rumble motors of a pad on USB:
/// `len` bytes (the device's output report length). A PlayStation 5 pad
/// takes id 0x02, then the flags that say the motors are given (compatible
/// vibration, haptics select) and the right (high frequency) and left (low
/// frequency) motor; a DualShock 4 takes id 0x05, the flag for the motors,
/// two bytes, then the right and the left motor.
pub(super) fn rumble_report(model: Model, len: usize, low: u16, high: u16) -> Vec<u8> {
    let mut report = vec![0u8; len.max(6)];
    match model {
        Model::Ps5 => {
            report[0] = 0x02;
            report[1] = 0x01 | 0x02;
            report[3] = (high >> 8) as u8;
            report[4] = (low >> 8) as u8;
        }
        Model::Ps4 => {
            report[0] = 0x05;
            report[1] = 0x01;
            report[4] = (high >> 8) as u8;
            report[5] = (low >> 8) as u8;
        }
    }
    report
}

#[cfg(windows)]
pub(super) mod reader {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
        SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList,
        SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
    };
    use windows_sys::Win32::Devices::HumanInterfaceDevice::{
        HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HidD_FreePreparsedData,
        HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData, HidP_GetCaps,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
    };

    use super::{
        LEFT_SHOULDER, Link, Model, PadState, RIGHT_SHOULDER, model, parse, rumble_report,
    };

    /// Looking for a pad again this often while none is there.
    const LOOK_AGAIN: Duration = Duration::from_secs(1);

    /// A handle we close.
    struct Owned(HANDLE);
    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: a handle we opened and close once.
            unsafe { CloseHandle(self.0) };
        }
    }
    // SAFETY: a file handle may be used from any thread.
    unsafe impl Send for Owned {}
    // SAFETY: as above; the writes through it are serialised by a mutex.
    unsafe impl Sync for Owned {}

    /// The pad the reader thread last saw, and a handle to write to it.
    #[derive(Default)]
    struct Shared {
        pad: Option<PadState>,
        out: Option<(Arc<Owned>, usize, Model)>,
    }

    /// The reader thread's results.
    pub(in super::super) struct Reader {
        shared: Arc<Mutex<Shared>>,
    }

    impl Reader {
        /// Starts the thread that finds and reads the pad.
        pub(in super::super) fn start() -> Reader {
            let shared = Arc::new(Mutex::new(Shared::default()));
            let theirs = shared.clone();
            let _ = std::thread::Builder::new()
                .name("victory-dualsense".into())
                .spawn(move || run(&theirs));
            Reader { shared }
        }

        /// The pad's last state, `None` while no pad is there.
        pub(in super::super) fn pad(&self) -> Option<PadState> {
            self.shared.lock().unwrap_or_else(|e| e.into_inner()).pad
        }

        /// Sets the motors, if a pad on USB is there.
        pub(in super::super) fn rumble(&self, low: u16, high: u16) {
            let out = self
                .shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .out
                .clone();
            let Some((handle, len, model)) = out else {
                return;
            };
            let report = rumble_report(model, len, low, high);
            let mut written = 0;
            // SAFETY: an open handle and our own buffer of `report.len()`
            // bytes.
            unsafe {
                WriteFile(
                    handle.0,
                    report.as_ptr(),
                    report.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
        }
    }

    fn run(shared: &Mutex<Shared>) {
        let mut said = false;
        loop {
            let Some((path, model)) = find() else {
                std::thread::sleep(LOOK_AGAIN);
                continue;
            };
            let Some((handle, input, output)) =
                open(&path, GENERIC_READ | GENERIC_WRITE).or_else(|| open(&path, GENERIC_READ))
            else {
                std::thread::sleep(LOOK_AGAIN);
                continue;
            };
            let handle = Arc::new(handle);
            // A pad on USB sends 64-byte reports; over Bluetooth the device
            // reports the length of its longest (78, or 547 for a DualShock 4).
            let link = if input <= 64 {
                Link::Usb
            } else {
                Link::Bluetooth
            };
            if !said {
                tracing::info!(
                    ?model,
                    ?link,
                    "a PlayStation pad plays as the first pad while XInput has none"
                );
                said = true;
            }
            {
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                s.out = (link == Link::Usb && output > 0).then(|| (handle.clone(), output, model));
            }
            let mut buffer = vec![0u8; input.max(64)];
            let (mut last, mut told) = ([false; 4], [0u8; 4]);
            loop {
                let mut read = 0;
                // SAFETY: an open handle and our own buffer; a blocking read
                // (the handle is not overlapped).
                let ok = unsafe {
                    ReadFile(
                        handle.0,
                        buffer.as_mut_ptr(),
                        buffer.len() as u32,
                        &mut read,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 || read == 0 {
                    break;
                }
                if let Some(pad) = parse(model, link, &buffer[..read as usize]) {
                    // The first presses of the triggers and shoulders, with
                    // the report's first bytes, so that a pad whose
                    // reports differ can be told apart in a log.
                    let held = [
                        pad.left_trigger > 0.5,
                        pad.right_trigger > 0.5,
                        pad.buttons & LEFT_SHOULDER != 0,
                        pad.buttons & RIGHT_SHOULDER != 0,
                    ];
                    for (i, (&now, was)) in held.iter().zip(&mut last).enumerate() {
                        if now && !*was && told[i] < 3 {
                            told[i] += 1;
                            tracing::info!(
                                what = ["LT", "RT", "LB", "RB"][i],
                                left_trigger = pad.left_trigger,
                                right_trigger = pad.right_trigger,
                                bytes = ?&buffer[..read.min(12) as usize],
                                "PlayStation pad: pressed"
                            );
                        }
                        *was = now;
                    }
                    shared.lock().unwrap_or_else(|e| e.into_inner()).pad = Some(pad);
                }
            }
            tracing::info!(?model, "the PlayStation pad is gone");
            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
            s.pad = None;
            s.out = None;
        }
    }

    /// Opens `path` with `access`: the handle and the input and output
    /// report lengths.
    fn open(path: &[u16], access: u32) -> Option<(Owned, usize, usize)> {
        // SAFETY: a NUL-terminated path; plain creation.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return None;
        }
        let handle = Owned(handle);
        let mut data = 0;
        // SAFETY: an open HID handle; the data is freed below.
        if !unsafe { HidD_GetPreparsedData(handle.0, &mut data) } {
            return None;
        }
        // SAFETY: plain data, filled by the call.
        let mut caps: HIDP_CAPS = unsafe { std::mem::zeroed() };
        // SAFETY: the preparsed data from above, freed once.
        let status = unsafe { HidP_GetCaps(data, &mut caps) };
        // SAFETY: as above.
        unsafe { HidD_FreePreparsedData(data) };
        if status != HIDP_STATUS_SUCCESS {
            return None;
        }
        Some((
            handle,
            usize::from(caps.InputReportByteLength),
            usize::from(caps.OutputReportByteLength),
        ))
    }

    /// The path of the first PlayStation pad present, NUL-terminated, and
    /// its model.
    fn find() -> Option<(Vec<u16>, Model)> {
        // SAFETY: plain data, filled by the call.
        let mut hid: windows_sys::core::GUID = unsafe { std::mem::zeroed() };
        // SAFETY: room for the GUID.
        unsafe { HidD_GetHidGuid(&mut hid) };
        // SAFETY: the HID class's GUID; the set is destroyed below.
        let set = unsafe {
            SetupDiGetClassDevsW(
                &hid,
                std::ptr::null(),
                std::ptr::null_mut(),
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            )
        };
        if set == -1 {
            return None;
        }
        let mut found = None;
        for index in 0.. {
            let mut interface = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            // SAFETY: the set from above; room for the interface data.
            if unsafe {
                SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &hid, index, &mut interface)
            } == 0
            {
                break;
            }
            let mut needed = 0;
            // SAFETY: a size query (no buffer).
            unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    set,
                    &interface,
                    std::ptr::null_mut(),
                    0,
                    &mut needed,
                    std::ptr::null_mut(),
                )
            };
            if (needed as usize) < size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() {
                continue;
            }
            // 8-byte aligned room for the detail, path included.
            let mut room = vec![0u64; (needed as usize).div_ceil(8)];
            let detail = room
                .as_mut_ptr()
                .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            // SAFETY: `room` holds at least the fixed part.
            unsafe { (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32 };
            // SAFETY: `needed` bytes of room, as asked.
            if unsafe {
                SetupDiGetDeviceInterfaceDetailW(
                    set,
                    &interface,
                    detail,
                    needed,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } == 0
            {
                continue;
            }
            // The path runs from `DevicePath` to its NUL, inside `room`.
            // SAFETY: the call wrote a NUL-terminated path there.
            let path: Vec<u16> = unsafe {
                let start = std::ptr::addr_of!((*detail).DevicePath).cast::<u16>();
                let len = (0..).take_while(|&i| *start.add(i) != 0).count();
                let mut path = std::slice::from_raw_parts(start, len).to_vec();
                path.push(0);
                path
            };
            if let Some(model) = pad_model(&path) {
                found = Some((path, model));
                break;
            }
        }
        // SAFETY: the set from above, destroyed once.
        unsafe { SetupDiDestroyDeviceInfoList(set) };
        found
    }

    /// The model of the PlayStation pad at `path`, if it is one (asked
    /// without read or write access, which every HID device allows).
    fn pad_model(path: &[u16]) -> Option<Model> {
        // SAFETY: a NUL-terminated path; plain creation.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return None;
        }
        let handle = Owned(handle);
        let mut attributes = HIDD_ATTRIBUTES {
            Size: size_of::<HIDD_ATTRIBUTES>() as u32,
            ..Default::default()
        };
        // SAFETY: an open HID handle; room for the attributes.
        let known = unsafe { HidD_GetAttributes(handle.0, &mut attributes) };
        if !known {
            return None;
        }
        model(attributes.VendorID, attributes.ProductID)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A USB report: sticks centred, triggers released, d-pad released.
    fn usb() -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[0] = 0x01;
        r[1..5].copy_from_slice(&[128, 128, 128, 128]);
        r[8] = 0x08;
        r
    }

    #[test]
    fn a_resting_pad_reads_as_nothing_pressed() {
        let pad = parse(Model::Ps5, Link::Usb, &usb()).unwrap();
        assert_eq!(pad.buttons, 0);
        assert_eq!(
            (pad.left_x, pad.left_y, pad.right_x, pad.right_y),
            (0.0, 0.0, 0.0, 0.0)
        );
        assert_eq!((pad.left_trigger, pad.right_trigger), (0.0, 0.0));
    }

    #[test]
    fn buttons_land_where_an_xbox_pad_has_them() {
        let mut r = usb();
        r[8] = 0x20 | 0x80 | 0x02; // cross, triangle, d-pad right
        r[9] = 0x01 | 0x20 | 0x40; // L1, Options, L3
        r[10] = 0x02; // touchpad click
        let pad = parse(Model::Ps5, Link::Usb, &r).unwrap();
        assert_eq!(
            pad.buttons,
            A | Y | RIGHT | LEFT_SHOULDER | MENU | LEFT_STICK | VIEW
        );
    }

    #[test]
    fn the_hat_gives_diagonals() {
        let mut r = usb();
        r[8] = 7;
        assert_eq!(parse(Model::Ps5, Link::Usb, &r).unwrap().buttons, UP | LEFT);
        r[8] = 3;
        assert_eq!(
            parse(Model::Ps5, Link::Usb, &r).unwrap().buttons,
            DOWN | RIGHT
        );
    }

    #[test]
    fn sticks_are_up_positive_and_triggers_run_to_one() {
        let mut r = usb();
        r[1] = 255; // left stick right
        r[2] = 0; // left stick up
        r[4] = 255; // right stick down
        r[5] = 255;
        r[6] = 0;
        let pad = parse(Model::Ps5, Link::Usb, &r).unwrap();
        assert_eq!((pad.left_x, pad.left_y, pad.right_y), (1.0, 1.0, -1.0));
        assert_eq!((pad.left_trigger, pad.right_trigger), (1.0, 0.0));
    }

    #[test]
    fn bluetooth_reports_of_both_kinds() {
        // Full: the USB layout one byte later.
        let mut full = vec![0u8; 78];
        full[0] = 0x31;
        full[2..6].copy_from_slice(&[128, 0, 128, 128]);
        full[9] = 0x08 | 0x40; // no d-pad, circle
        let pad = parse(Model::Ps5, Link::Bluetooth, &full).unwrap();
        assert_eq!(pad.buttons, B);
        assert_eq!(pad.left_y, 1.0);
        // Short: sticks, buttons, then triggers.
        let mut short = vec![0u8; 78];
        short[0] = 0x01;
        short[1..5].copy_from_slice(&[128, 128, 128, 128]);
        short[5] = 0x08 | 0x10; // square
        short[8] = 255;
        let pad = parse(Model::Ps5, Link::Bluetooth, &short).unwrap();
        assert_eq!(pad.buttons, X);
        assert_eq!(pad.left_trigger, 1.0);
    }

    #[test]
    fn other_reports_are_not_input() {
        assert!(parse(Model::Ps5, Link::Usb, &[0x05, 0, 0]).is_none());
        assert!(parse(Model::Ps5, Link::Usb, &[0x01, 128]).is_none());
        assert!(parse(Model::Ps5, Link::Bluetooth, &[0x02; 20]).is_none());
    }

    #[test]
    fn rumble_reports_carry_both_motors() {
        let r = rumble_report(Model::Ps5, 48, 0xFF00, 0x8000);
        assert_eq!(r.len(), 48);
        assert_eq!(&r[..5], &[0x02, 0x03, 0x00, 0x80, 0xFF]);
        let r = rumble_report(Model::Ps4, 32, 0xFF00, 0x8000);
        assert_eq!(r.len(), 32);
        assert_eq!(&r[..6], &[0x05, 0x01, 0x00, 0x00, 0x80, 0xFF]);
    }

    /// A DualShock 4 report: sticks, buttons, then triggers, from `at`.
    fn ps4(id: u8, at: usize) -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[0] = id;
        r[at..at + 4].copy_from_slice(&[128, 128, 128, 128]);
        r[at + 4] = 0x08;
        r
    }

    #[test]
    fn a_dualshock_4_reads_on_usb_and_bluetooth() {
        let mut usb = ps4(0x01, 1);
        usb[5] = 0x08 | 0x20; // cross
        usb[6] = 0x02 | 0x10; // R1, Share
        usb[9] = 255; // R2
        let pad = parse(Model::Ps4, Link::Usb, &usb).unwrap();
        assert_eq!(pad.buttons, A | RIGHT_SHOULDER | VIEW);
        assert_eq!((pad.left_trigger, pad.right_trigger), (0.0, 1.0));
        // The short report over Bluetooth is the same.
        assert_eq!(parse(Model::Ps4, Link::Bluetooth, &usb), Some(pad));
        // The full one, two bytes later.
        let mut full = ps4(0x11, 3);
        full[4] = 0; // left stick up
        full[7] = 0x06 | 0x80; // d-pad left, triangle
        full[10] = 255; // L2
        let pad = parse(Model::Ps4, Link::Bluetooth, &full).unwrap();
        assert_eq!(pad.buttons, LEFT | Y);
        assert_eq!((pad.left_y, pad.left_trigger), (1.0, 1.0));
        // Not a DualSense's USB layout.
        assert_ne!(
            parse(Model::Ps5, Link::Usb, &usb),
            parse(Model::Ps4, Link::Usb, &usb)
        );
    }

    #[test]
    fn sony_pads_are_told_apart_by_product() {
        assert_eq!(model(VENDOR, 0x0CE6), Some(Model::Ps5));
        assert_eq!(model(VENDOR, 0x09CC), Some(Model::Ps4));
        assert_eq!(model(VENDOR, 0x05C4), Some(Model::Ps4));
        assert_eq!(model(0x045E, 0x09CC), None);
    }
}
