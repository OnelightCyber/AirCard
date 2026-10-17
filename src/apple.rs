use std::ffi::{CStr, CString};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, bail};
use libloading::{Library, Symbol};

unsafe extern "system" {
    fn SetDllDirectoryW(lpPathName: *const u16) -> i32;
    fn CreateToolhelp32Snapshot(dw_flags: u32, th32_process_id: u32) -> *mut std::ffi::c_void;
    fn Process32FirstW(h_snapshot: *mut std::ffi::c_void, lppe: *mut Processentry32W) -> i32;
    fn Process32NextW(h_snapshot: *mut std::ffi::c_void, lppe: *mut Processentry32W) -> i32;
    fn CloseHandle(h_object: *mut std::ffi::c_void) -> i32;
    fn OpenProcess(
        dw_desired_access: u32,
        b_inherit_handle: i32,
        dw_process_id: u32,
    ) -> *mut std::ffi::c_void;
    fn QueryFullProcessImageNameW(
        h_process: *mut std::ffi::c_void,
        dw_flags: u32,
        lp_exe_name: *mut u16,
        lpdw_size: *mut u32,
    ) -> i32;
}

const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x0000_1000;

#[repr(C)]
struct Processentry32W {
    dw_size: u32,
    cnt_usage: u32,
    th32_process_id: u32,
    th32_default_heap_id: usize,
    th32_module_id: u32,
    cnt_threads: u32,
    th32_parent_process_id: u32,
    pc_pri_class_base: i32,
    dw_flags: u32,
    sz_exe_file: [u16; 260],
}

const APPLE_SUPPORT_DIRS: &[&str] = &[
    r"C:\Program Files\Common Files\Apple\Mobile Device Support",
    r"C:\Program Files (x86)\Common Files\Apple\Mobile Device Support",
];

/// Full image path of a running process matching `exe_name` (case-insensitive), if any.
/// Used to locate Apple's DLLs inside the Microsoft Store package (Apple Devices / iTunes),
/// which does not create the classic `Common Files\Apple\Mobile Device Support` directory.
fn find_process_image_path(exe_name: &str) -> Option<PathBuf> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot.is_null() || snapshot as isize == -1 {
            return None;
        }

        let mut entry: Processentry32W = std::mem::zeroed();
        entry.dw_size = std::mem::size_of::<Processentry32W>() as u32;

        let mut pid: Option<u32> = None;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let end = entry
                    .sz_exe_file
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.sz_exe_file.len());
                let name = String::from_utf16_lossy(&entry.sz_exe_file[..end]);
                if name.eq_ignore_ascii_case(exe_name) {
                    pid = Some(entry.th32_process_id);
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);

        let pid = pid?;
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }

        let mut buffer = vec![0u16; 32_768];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }

        Some(PathBuf::from(String::from_utf16_lossy(&buffer[..size as usize])))
    }
}

/// The three Apple DLLs AirCard links against. A directory is a valid support dir
/// only when it holds all three and we can load them from it.
const CORE_DLLS: [&str; 3] = [
    "CoreFoundation.dll",
    "MobileDevice.dll",
    "AirTrafficHost.dll",
];

/// Minimum Apple runtime needed by MobileDevice/AirTrafficHost, used as a fallback
/// when the package directory cannot be listed. Missing names are skipped silently.
const FALLBACK_DLLS: &[&str] = &[
    "CoreFoundation.dll", "MobileDevice.dll", "AirTrafficHost.dll",
    "ASL.dll", "CFNetwork.dll", "CoreText.dll", "CoreGraphics.dll", "Foundation.dll",
    "QuartzCore.dll", "CoreVideo.dll", "CoreMedia.dll", "MediaAccessibility.dll",
    "AVFoundationCF.dll", "CoreAudioToolbox.dll", "objc.dll", "libdispatch.dll",
    "pthreadVC2.dll", "SQLite3.dll", "zlib1.dll", "zlib.dll", "icudt62.dll",
    "icui18n.dll", "icuuc.dll", "libicuin.dll", "libicuuc.dll", "dnssd.dll",
    "mDNSResponderDLL.dll", "AppleVersions.dll", "marshal.dll", "libxml2.dll",
    "libxslt.dll", "libtidy.dll", "CoreFP.dll", "CoreADI.dll", "CoreADI64.dll",
    "CoreKE.dll", "CoreLSKD.dll", "ApplePushDirect.dll", "AuthKitWin.dll",
    "AppleMediaServicesKit.dll", "crypto-44.dll", "ssl-46.dll", "JavaScriptCoreV8.dll",
    "AMPLibraryUtils.dll", "CoreMessagingXP.dll",
];

fn dir_has_core_dlls(dir: &std::path::Path) -> bool {
    CORE_DLLS.iter().all(|name| dir.join(name).is_file())
}

/// Directories AirCard can actually load DLLs from (normal ACLs, execute allowed):
/// classic Common Files, next to the exe, and our per-user provisioning dir.
fn loadable_candidate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = APPLE_SUPPORT_DIRS.iter().map(PathBuf::from).collect();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
            dirs.push(parent.join("MobileDeviceSupport"));
        }
    }

    dirs.push(provision_dir());
    dirs
}

/// Per-user directory we copy the Store DLLs into so they become loadable.
fn provision_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("AirCard").join("MobileDeviceSupport")
}

/// Source directory of the Apple DLLs inside the Microsoft Store package
/// ("Apple Devices" / iTunes), discovered from the live usbmux daemons.
/// These files are readable but NOT loadable in place (WindowsApps denies execute
/// to third-party processes), so they are only used as a copy source.
fn package_source_dir() -> Option<PathBuf> {
    for proc_name in [
        "AppleMobileDeviceProcess.exe",
        "AppleMobileDeviceService.exe",
        "iTunes.exe",
    ] {
        if let Some(path) = find_process_image_path(proc_name) {
            if let Some(parent) = path.parent() {
                if dir_has_core_dlls(parent) {
                    return Some(parent.to_path_buf());
                }
                let amds64 = parent.join("AMDS64");
                if dir_has_core_dlls(&amds64) {
                    return Some(amds64);
                }
            }
        }
    }
    None
}

/// Windows DLLs bundled in the Store package that the Apple stack does not need;
/// skipped to keep the per-user copy small.
fn is_skippable_dll(name: &str) -> bool {
    const SKIP_PREFIXES: &[&str] = &[
        "Microsoft.", "Windows", "onnxruntime", "DirectML", "v8", "WinUIEdit",
        "dcompi", "dwmcorei", "DwmSceneI", "wuceffectsi", "NPUDetect", "MRM.",
        "DWriteCore", "Session", "Push", "AMP.", "AMPLibraryAgent", "WindowsApp",
        "JetEngine", "iPodUpdater", "AppleMobileDeviceProcess_main",
    ];
    let lower = name.to_ascii_lowercase();
    SKIP_PREFIXES
        .iter()
        .any(|p| lower.starts_with(&p.to_ascii_lowercase()))
}

/// Copy the Apple DLL set from `src` into `dst`, overwriting stale copies.
/// Returns the number of DLLs present in `dst` afterward.
fn provision_support_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create {}", dst.display()))?;

    // Prefer copying every relevant DLL by listing the source directory.
    let mut copied_any = false;
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dll = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("dll"))
                .unwrap_or(false);
            if !is_dll {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if is_skippable_dll(&name) {
                continue;
            }
            if copy_if_newer(&path, &dst.join(&name)).is_ok() {
                copied_any = true;
            }
        }
    }

    // Fallback: the package dir could not be listed; copy a known-good subset by name.
    if !copied_any {
        for name in FALLBACK_DLLS {
            let from = src.join(name);
            if from.is_file() {
                let _ = copy_if_newer(&from, &dst.join(name));
            }
        }
    }

    if !dir_has_core_dlls(dst) {
        bail!(
            "Copied Apple runtime into {} but core DLLs are still missing",
            dst.display()
        );
    }
    Ok(())
}

fn copy_if_newer(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    if let (Ok(src_meta), Ok(dst_meta)) = (from.metadata(), to.metadata()) {
        if src_meta.len() == dst_meta.len() {
            return Ok(()); // already provisioned, same size
        }
    }
    std::fs::copy(from, to).map(|_| ())
}

pub type CFTypeRef = *const std::ffi::c_void;
pub type CFStringRef = *const std::ffi::c_void;
pub type CFDataRef = *const std::ffi::c_void;
pub type CFDictionaryRef = *const std::ffi::c_void;
pub type CFArrayRef = *const std::ffi::c_void;
pub type CFPropertyListRef = *const std::ffi::c_void;
pub type CFAllocatorRef = *const std::ffi::c_void;
pub type CFIndex = isize;
pub type CFStringEncoding = u32;

pub const K_CFSTRING_ENCODING_UTF8: CFStringEncoding = 0x08000100;
pub const K_CFPROPERTY_LIST_BINARY_FORMAT_V1_0: isize = 200;

pub type ATHostConnectionRef = *mut std::ffi::c_void;

pub type AMDeviceRef = *const std::ffi::c_void;
pub type AMDeviceNotificationRef = *const std::ffi::c_void;
pub type AMDServiceConnectionRef = *mut std::ffi::c_void;
pub type AFCConnectionRef = *mut std::ffi::c_void;
pub type AFCKeyValueRef = *mut std::ffi::c_void;
pub type AFCFileRef = u64;
pub type AFCDirectoryRef = *mut std::ffi::c_void;

#[repr(C)]
pub struct AMDeviceNotificationCallbackInfo {
    pub device: AMDeviceRef,
    pub message: u32,
}

pub type AMDeviceNotificationCallback =
    extern "C" fn(*const AMDeviceNotificationCallbackInfo, *mut std::ffi::c_void);

#[allow(dead_code)]
pub struct AppleLibraries {
    _cf_lib: Library,
    _md_lib: Library,
    _ath_lib: Library,

    // CoreFoundation functions
    pub cf_string_create: unsafe extern "C" fn(CFAllocatorRef, *const std::ffi::c_char, CFStringEncoding) -> CFStringRef,
    pub cf_string_get_length: unsafe extern "C" fn(CFStringRef) -> CFIndex,
    pub cf_string_get_max_size: unsafe extern "C" fn(CFIndex, CFStringEncoding) -> CFIndex,
    pub cf_string_get_c_string: unsafe extern "C" fn(CFStringRef, *mut std::ffi::c_char, CFIndex, CFStringEncoding) -> u8,
    pub cf_data_create: unsafe extern "C" fn(CFAllocatorRef, *const u8, CFIndex) -> CFDataRef,
    pub cf_data_get_byte_ptr: unsafe extern "C" fn(CFDataRef) -> *const u8,
    pub cf_data_get_length: unsafe extern "C" fn(CFDataRef) -> CFIndex,
    pub cf_property_list_create_with_data: unsafe extern "C" fn(CFAllocatorRef, CFDataRef, usize, *mut isize, *mut CFTypeRef) -> CFPropertyListRef,
    pub cf_property_list_create_data: unsafe extern "C" fn(CFAllocatorRef, CFPropertyListRef, isize, usize, *mut CFTypeRef) -> CFDataRef,
    pub cf_release: unsafe extern "C" fn(CFTypeRef),
    pub cf_retain: unsafe extern "C" fn(CFTypeRef) -> CFTypeRef,
    pub cf_equal: unsafe extern "C" fn(CFTypeRef, CFTypeRef) -> u32,
    pub cf_run_loop_get_main: unsafe extern "C" fn() -> *const std::ffi::c_void,
    pub cf_run_loop_run_in_mode: unsafe extern "C" fn(CFStringRef, f64, u8) -> i32,
    pub cf_run_loop_stop: unsafe extern "C" fn(*const std::ffi::c_void),

    // MobileDevice functions
    pub am_device_create_from_properties: unsafe extern "C" fn(CFDictionaryRef) -> AMDeviceRef,
    pub am_device_notification_subscribe: unsafe extern "C" fn(
        AMDeviceNotificationCallback,
        i32,
        u32,
        *mut std::ffi::c_void,
        *mut AMDeviceNotificationRef,
        CFDictionaryRef,
    ) -> i32,
    pub am_device_notification_unsubscribe: unsafe extern "C" fn(AMDeviceNotificationRef) -> i32,
    pub am_device_copy_device_identifier: unsafe extern "C" fn(AMDeviceRef) -> CFStringRef,
    pub am_device_copy_value: unsafe extern "C" fn(AMDeviceRef, CFStringRef, CFStringRef) -> CFTypeRef,
    pub am_device_connect: unsafe extern "C" fn(AMDeviceRef) -> i32,
    pub am_device_disconnect: unsafe extern "C" fn(AMDeviceRef) -> i32,
    pub am_device_is_paired: unsafe extern "C" fn(AMDeviceRef) -> i32,
    pub am_device_pair: unsafe extern "C" fn(AMDeviceRef) -> i32,
    pub am_device_validate_pairing: unsafe extern "C" fn(AMDeviceRef) -> i32,
    pub am_device_start_session: unsafe extern "C" fn(AMDeviceRef) -> i32,
    pub am_device_stop_session: unsafe extern "C" fn(AMDeviceRef) -> i32,
    pub am_device_secure_start_service: unsafe extern "C" fn(
        AMDeviceRef,
        CFStringRef,
        CFDictionaryRef,
        *mut AMDServiceConnectionRef,
    ) -> i32,
    pub amd_service_connection_get_socket: unsafe extern "C" fn(AMDServiceConnectionRef) -> i32,
    pub amd_service_connection_get_secure_io_context: unsafe extern "C" fn(AMDServiceConnectionRef) -> *mut std::ffi::c_void,
    pub amd_service_connection_invalidate: unsafe extern "C" fn(AMDServiceConnectionRef) -> i32,
    pub amd_service_connection_send: unsafe extern "C" fn(AMDServiceConnectionRef, *const u8, usize) -> i32,
    pub amd_service_connection_receive: unsafe extern "C" fn(AMDServiceConnectionRef, *mut u8, usize) -> i32,
    pub amd_service_connection_send_message: unsafe extern "C" fn(AMDServiceConnectionRef, CFTypeRef, isize) -> i32,
    pub amd_service_connection_receive_message: unsafe extern "C" fn(AMDServiceConnectionRef, *mut CFTypeRef, *mut isize) -> i32,

    // AFC functions
    pub afc_connection_open: unsafe extern "C" fn(i32, u32, *mut AFCConnectionRef) -> i32,
    pub afc_connection_close: unsafe extern "C" fn(AFCConnectionRef) -> i32,
    pub afc_connection_set_secure_context: unsafe extern "C" fn(AFCConnectionRef, *mut std::ffi::c_void) -> i32,
    pub afc_connection_set_dispose_secure_context: unsafe extern "C" fn(AFCConnectionRef, i32) -> i32,
    pub afc_connection_set_io_timeout: unsafe extern "C" fn(AFCConnectionRef, u32) -> i32,
    pub afc_file_info_open: unsafe extern "C" fn(AFCConnectionRef, *const std::ffi::c_char, *mut AFCKeyValueRef) -> i32,
    pub afc_key_value_read: unsafe extern "C" fn(AFCKeyValueRef, *mut *const std::ffi::c_char, *mut *const std::ffi::c_char) -> i32,
    pub afc_key_value_close: unsafe extern "C" fn(AFCKeyValueRef) -> i32,
    pub afc_file_ref_open: unsafe extern "C" fn(AFCConnectionRef, *const std::ffi::c_char, u64, *mut AFCFileRef) -> i32,
    pub afc_file_ref_read: unsafe extern "C" fn(AFCConnectionRef, AFCFileRef, *mut u8, *mut isize) -> i32,
    pub afc_file_ref_write: unsafe extern "C" fn(AFCConnectionRef, AFCFileRef, *const u8, isize) -> i32,
    pub afc_file_ref_close: unsafe extern "C" fn(AFCConnectionRef, AFCFileRef) -> i32,
    pub afc_directory_open: unsafe extern "C" fn(AFCConnectionRef, *const std::ffi::c_char, *mut AFCDirectoryRef) -> i32,
    pub afc_directory_read: unsafe extern "C" fn(AFCConnectionRef, AFCDirectoryRef, *mut *const std::ffi::c_char) -> i32,
    pub afc_directory_close: unsafe extern "C" fn(AFCConnectionRef, AFCDirectoryRef) -> i32,
    pub afc_directory_create: unsafe extern "C" fn(AFCConnectionRef, *const std::ffi::c_char) -> i32,
    pub afc_remove_path: unsafe extern "C" fn(AFCConnectionRef, *const std::ffi::c_char) -> i32,

    // AirTrafficHost functions
    pub at_host_connection_create: unsafe extern "C" fn(CFStringRef) -> ATHostConnectionRef,
    pub at_host_connection_release: unsafe extern "C" fn(ATHostConnectionRef),
    pub at_host_connection_send_host_info: unsafe extern "C" fn(ATHostConnectionRef, CFDictionaryRef),
    pub at_host_connection_send_sync_request: unsafe extern "C" fn(ATHostConnectionRef, CFArrayRef, CFDictionaryRef, CFDictionaryRef),
    pub at_host_connection_send_metadata_sync_finished: unsafe extern "C" fn(ATHostConnectionRef, CFDictionaryRef, CFDictionaryRef),
    pub at_host_connection_send_asset_completed: unsafe extern "C" fn(ATHostConnectionRef, CFStringRef, CFStringRef, CFStringRef),
    pub at_host_connection_read_message: unsafe extern "C" fn(ATHostConnectionRef) -> CFDictionaryRef,
    pub at_cf_message_get_name: unsafe extern "C" fn(CFDictionaryRef) -> CFStringRef,
    pub at_cf_message_get_param: unsafe extern "C" fn(CFDictionaryRef, CFStringRef) -> CFTypeRef,
}

static LIBRARIES: OnceLock<Arc<AppleLibraries>> = OnceLock::new();

pub fn locate_support_dir() -> Option<PathBuf> {
    // 1. A directory we can load from directly (classic iTunes, next to exe, or a
    //    previous provisioning).
    if let Some(dir) = loadable_candidate_dirs().into_iter().find(|d| dir_has_core_dlls(d)) {
        return Some(dir);
    }

    // 2. Only the Microsoft Store package is present. Its DLLs are readable but not
    //    loadable in place, so copy them once into a per-user directory and use that.
    if let Some(src) = package_source_dir() {
        let dst = provision_dir();
        match provision_support_dir(&src, &dst) {
            Ok(()) => return Some(dst),
            Err(err) => eprintln!("AirCard: DLL provisioning failed: {err:#}"),
        }
    }

    None
}

pub fn get_apple_libraries() -> Result<Arc<AppleLibraries>> {
    if let Some(libs) = LIBRARIES.get() {
        return Ok(Arc::clone(libs));
    }

    let dir = locate_support_dir().context(
        "Apple Mobile Device Support DLLs not found. Install the 'Apple Devices' app (Microsoft Store) or 64-bit iTunes, or place CoreFoundation.dll, MobileDevice.dll and AirTrafficHost.dll next to aircard.exe.",
    )?;

    // Configure Windows DLL search directory so dependent DLLs (e.g. objc, pthread, SQLite) are resolved
    let wide_dir: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        SetDllDirectoryW(wide_dir.as_ptr());
    }

    let cf_path = dir.join("CoreFoundation.dll");
    let md_path = dir.join("MobileDevice.dll");
    let ath_path = dir.join("AirTrafficHost.dll");

    unsafe {
        let cf_lib = Library::new(&cf_path).context("Failed to load CoreFoundation.dll")?;
        let md_lib = Library::new(&md_path).context("Failed to load MobileDevice.dll")?;
        let ath_lib = Library::new(&ath_path).context("Failed to load AirTrafficHost.dll")?;

        macro_rules! load_sym {
            ($lib:expr, $name:expr) => {{
                let symbol: Symbol<_> = $lib
                    .get($name.as_bytes())
                    .context(concat!("Missing symbol: ", $name))?;
                *symbol
            }};
        }

        let cf_string_create = load_sym!(cf_lib, "CFStringCreateWithCString");
        let cf_string_get_length = load_sym!(cf_lib, "CFStringGetLength");
        let cf_string_get_max_size = load_sym!(cf_lib, "CFStringGetMaximumSizeForEncoding");
        let cf_string_get_c_string = load_sym!(cf_lib, "CFStringGetCString");
        let cf_data_create = load_sym!(cf_lib, "CFDataCreate");
        let cf_data_get_byte_ptr = load_sym!(cf_lib, "CFDataGetBytePtr");
        let cf_data_get_length = load_sym!(cf_lib, "CFDataGetLength");
        let cf_property_list_create_with_data = load_sym!(cf_lib, "CFPropertyListCreateWithData");
        let cf_property_list_create_data = load_sym!(cf_lib, "CFPropertyListCreateData");
        let cf_release = load_sym!(cf_lib, "CFRelease");
        let cf_retain = load_sym!(cf_lib, "CFRetain");
        let cf_equal = load_sym!(cf_lib, "CFEqual");
        let cf_run_loop_get_main = load_sym!(cf_lib, "CFRunLoopGetMain");
        let cf_run_loop_run_in_mode = load_sym!(cf_lib, "CFRunLoopRunInMode");
        let cf_run_loop_stop = load_sym!(cf_lib, "CFRunLoopStop");

        let am_device_create_from_properties = load_sym!(md_lib, "AMDeviceCreateFromProperties");
        let am_device_notification_subscribe = load_sym!(md_lib, "AMDeviceNotificationSubscribeWithOptions");
        let am_device_notification_unsubscribe = load_sym!(md_lib, "AMDeviceNotificationUnsubscribe");
        let am_device_copy_device_identifier = load_sym!(md_lib, "AMDeviceCopyDeviceIdentifier");
        let am_device_copy_value = load_sym!(md_lib, "AMDeviceCopyValue");
        let am_device_connect = load_sym!(md_lib, "AMDeviceConnect");
        let am_device_disconnect = load_sym!(md_lib, "AMDeviceDisconnect");
        let am_device_is_paired = load_sym!(md_lib, "AMDeviceIsPaired");
        let am_device_pair = load_sym!(md_lib, "AMDevicePair");
        let am_device_validate_pairing = load_sym!(md_lib, "AMDeviceValidatePairing");
        let am_device_start_session = load_sym!(md_lib, "AMDeviceStartSession");
        let am_device_stop_session = load_sym!(md_lib, "AMDeviceStopSession");
        let am_device_secure_start_service = load_sym!(md_lib, "AMDeviceSecureStartService");
        let amd_service_connection_get_socket = load_sym!(md_lib, "AMDServiceConnectionGetSocket");
        let amd_service_connection_get_secure_io_context = load_sym!(md_lib, "AMDServiceConnectionGetSecureIOContext");
        let amd_service_connection_invalidate = load_sym!(md_lib, "AMDServiceConnectionInvalidate");
        let amd_service_connection_send = load_sym!(md_lib, "AMDServiceConnectionSend");
        let amd_service_connection_receive = load_sym!(md_lib, "AMDServiceConnectionReceive");
        let amd_service_connection_send_message = load_sym!(md_lib, "AMDServiceConnectionSendMessage");
        let amd_service_connection_receive_message = load_sym!(md_lib, "AMDServiceConnectionReceiveMessage");

        let afc_connection_open = load_sym!(md_lib, "AFCConnectionOpen");
        let afc_connection_close = load_sym!(md_lib, "AFCConnectionClose");
        let afc_connection_set_secure_context = load_sym!(md_lib, "AFCConnectionSetSecureContext");
        let afc_connection_set_dispose_secure_context = load_sym!(md_lib, "AFCConnectionSetDisposeSecureContextOnInvalidate");
        let afc_connection_set_io_timeout = load_sym!(md_lib, "AFCConnectionSetIOTimeout");
        let afc_file_info_open = load_sym!(md_lib, "AFCFileInfoOpen");
        let afc_key_value_read = load_sym!(md_lib, "AFCKeyValueRead");
        let afc_key_value_close = load_sym!(md_lib, "AFCKeyValueClose");
        let afc_file_ref_open = load_sym!(md_lib, "AFCFileRefOpen");
        let afc_file_ref_read = load_sym!(md_lib, "AFCFileRefRead");
        let afc_file_ref_write = load_sym!(md_lib, "AFCFileRefWrite");
        let afc_file_ref_close = load_sym!(md_lib, "AFCFileRefClose");
        let afc_directory_open = load_sym!(md_lib, "AFCDirectoryOpen");
        let afc_directory_read = load_sym!(md_lib, "AFCDirectoryRead");
        let afc_directory_close = load_sym!(md_lib, "AFCDirectoryClose");
        let afc_directory_create = load_sym!(md_lib, "AFCDirectoryCreate");
        let afc_remove_path = load_sym!(md_lib, "AFCRemovePath");

        let at_host_connection_create = load_sym!(ath_lib, "ATHostConnectionCreate");
        let at_host_connection_release = load_sym!(ath_lib, "ATHostConnectionRelease");
        let at_host_connection_send_host_info = load_sym!(ath_lib, "ATHostConnectionSendHostInfo");
        let at_host_connection_send_sync_request = load_sym!(ath_lib, "ATHostConnectionSendSyncRequest");
        let at_host_connection_send_metadata_sync_finished = load_sym!(ath_lib, "ATHostConnectionSendMetadataSyncFinished");
        let at_host_connection_send_asset_completed = load_sym!(ath_lib, "ATHostConnectionSendAssetCompleted");
        let at_host_connection_read_message = load_sym!(ath_lib, "ATHostConnectionReadMessage");
        let at_cf_message_get_name = load_sym!(ath_lib, "ATCFMessageGetName");
        let at_cf_message_get_param = load_sym!(ath_lib, "ATCFMessageGetParam");

        let apple_libs = Arc::new(AppleLibraries {
            _cf_lib: cf_lib,
            _md_lib: md_lib,
            _ath_lib: ath_lib,

            cf_string_create,
            cf_string_get_length,
            cf_string_get_max_size,
            cf_string_get_c_string,
            cf_data_create,
            cf_data_get_byte_ptr,
            cf_data_get_length,
            cf_property_list_create_with_data,
            cf_property_list_create_data,
            cf_release,
            cf_retain,
            cf_equal,
            cf_run_loop_get_main,
            cf_run_loop_run_in_mode,
            cf_run_loop_stop,

            am_device_create_from_properties,
            am_device_notification_subscribe,
            am_device_notification_unsubscribe,
            am_device_copy_device_identifier,
            am_device_copy_value,
            am_device_connect,
            am_device_disconnect,
            am_device_is_paired,
            am_device_pair,
            am_device_validate_pairing,
            am_device_start_session,
            am_device_stop_session,
            am_device_secure_start_service,
            amd_service_connection_get_socket,
            amd_service_connection_get_secure_io_context,
            amd_service_connection_invalidate,
            amd_service_connection_send,
            amd_service_connection_receive,
            amd_service_connection_send_message,
            amd_service_connection_receive_message,

            afc_connection_open,
            afc_connection_close,
            afc_connection_set_secure_context,
            afc_connection_set_dispose_secure_context,
            afc_connection_set_io_timeout,
            afc_file_info_open,
            afc_key_value_read,
            afc_key_value_close,
            afc_file_ref_open,
            afc_file_ref_read,
            afc_file_ref_write,
            afc_file_ref_close,
            afc_directory_open,
            afc_directory_read,
            afc_directory_close,
            afc_directory_create,
            afc_remove_path,

            at_host_connection_create,
            at_host_connection_release,
            at_host_connection_send_host_info,
            at_host_connection_send_sync_request,
            at_host_connection_send_metadata_sync_finished,
            at_host_connection_send_asset_completed,
            at_host_connection_read_message,
            at_cf_message_get_name,
            at_cf_message_get_param,
        });

        let _ = LIBRARIES.set(Arc::clone(&apple_libs));
        Ok(apple_libs)
    }
}

pub struct CFStringGuard {
    pub raw: CFStringRef,
    libs: Arc<AppleLibraries>,
}

impl Drop for CFStringGuard {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                (self.libs.cf_release)(self.raw);
            }
        }
    }
}

pub struct CFTypeGuard {
    pub raw: CFTypeRef,
    libs: Arc<AppleLibraries>,
}

impl Drop for CFTypeGuard {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe {
                (self.libs.cf_release)(self.raw);
            }
        }
    }
}

impl AppleLibraries {
    pub fn create_cf_string(&self, s: &str) -> Result<CFStringGuard> {
        let c_str = CString::new(s).context("String contains null byte")?;
        let raw = unsafe {
            (self.cf_string_create)(ptr::null(), c_str.as_ptr(), K_CFSTRING_ENCODING_UTF8)
        };
        if raw.is_null() {
            bail!("Failed to create CFString for {}", s);
        }
        Ok(CFStringGuard {
            raw,
            libs: get_apple_libraries()?,
        })
    }

    pub fn to_rust_string(&self, cf_str: CFStringRef) -> String {
        if cf_str.is_null() {
            return String::new();
        }
        unsafe {
            let length = (self.cf_string_get_length)(cf_str);
            let max_bytes = (self.cf_string_get_max_size)(length, K_CFSTRING_ENCODING_UTF8) + 1;
            let mut buffer: Vec<u8> = vec![0; max_bytes as usize];
            if (self.cf_string_get_c_string)(
                cf_str,
                buffer.as_mut_ptr() as *mut std::ffi::c_char,
                max_bytes,
                K_CFSTRING_ENCODING_UTF8,
            ) != 0
            {
                if let Ok(c_str) = CStr::from_ptr(buffer.as_ptr() as *const std::ffi::c_char).to_str() {
                    return c_str.to_owned();
                }
            }
        }
        String::new()
    }

    pub fn create_cf_plist_from_bytes(&self, bytes: &[u8]) -> Result<CFTypeGuard> {
        unsafe {
            let cf_data = (self.cf_data_create)(ptr::null(), bytes.as_ptr(), bytes.len() as isize);
            if cf_data.is_null() {
                bail!("Failed to create CFData");
            }
            let plist = (self.cf_property_list_create_with_data)(
                ptr::null(),
                cf_data,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            (self.cf_release)(cf_data);
            if plist.is_null() {
                bail!("Failed to parse plist data into CFPropertyList");
            }
            Ok(CFTypeGuard {
                raw: plist,
                libs: get_apple_libraries()?,
            })
        }
    }

    pub fn cf_plist_to_bytes(&self, plist: CFPropertyListRef) -> Result<Vec<u8>> {
        if plist.is_null() {
            bail!("Null CFPropertyList");
        }
        unsafe {
            let cf_data = (self.cf_property_list_create_data)(
                ptr::null(),
                plist,
                K_CFPROPERTY_LIST_BINARY_FORMAT_V1_0,
                0,
                ptr::null_mut(),
            );
            if cf_data.is_null() {
                bail!("Failed to serialize CFPropertyList to binary data");
            }
            let ptr = (self.cf_data_get_byte_ptr)(cf_data);
            let len = (self.cf_data_get_length)(cf_data) as usize;
            let slice = std::slice::from_raw_parts(ptr, len);
            let result = slice.to_vec();
            (self.cf_release)(cf_data);
            Ok(result)
        }
    }
}

pub fn verify_support() -> Result<String> {
    let _libs = get_apple_libraries()?;
    let dir = locate_support_dir().unwrap_or_default();
    Ok(format!("Apple Mobile Device Support ready: {}", dir.display()))
}
