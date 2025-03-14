#![no_std]
#![no_main]
#![feature(abi_efiapi)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate alloc;

use alloc::string::String;
use uefi::prelude::*;
use uefi::proto::console::serial::Serial;
use uefi::table::boot::{OpenProtocolAttributes, OpenProtocolParams};

mod boot;
mod proto;
mod runtime;

#[entry]
fn efi_main(image: Handle, mut st: SystemTable<Boot>) -> Status {
    // Initialize utilities (logging, memory allocation...)
    uefi_services::init(&mut st).expect_success("Failed to initialize utilities");

    // unit tests here

    // output firmware-vendor (CStr16 to Rust string)
    let mut buf = String::new();
    st.firmware_vendor().as_str_in_buf(&mut buf).unwrap();
    info!("Firmware Vendor: {}", buf.as_str());

    // Reset the console before running all the other tests.
    st.stdout()
        .reset(false)
        .expect_success("Failed to reset stdout");

    // Ensure the tests are run on a version of UEFI we support.
    check_revision(st.uefi_revision());

    // Test all the boot services.
    let bt = st.boot_services();

    // Try retrieving a handle to the file system the image was booted from.
    bt.get_image_file_system(image)
        .expect("Failed to retrieve boot file system")
        .unwrap();

    boot::test(bt);

    // Test all the supported protocols.
    proto::test(image, &mut st);

    // TODO: runtime services work before boot services are exited, but we'd
    // probably want to test them after exit_boot_services. However,
    // exit_boot_services is currently called during shutdown.

    runtime::test(st.runtime_services());

    shutdown(image, st);
}

fn check_revision(rev: uefi::table::Revision) {
    let (major, minor) = (rev.major(), rev.minor());

    info!("UEFI {}.{}", major, minor / 10);

    assert!(major >= 2, "Running on an old, unsupported version of UEFI");
    assert!(
        minor >= 30,
        "Old version of UEFI 2, some features might not be available."
    );
}

/// Ask the test runner to check the current screen output against a reference
///
/// This functionality is very specific to our QEMU-based test runner. Outside
/// of it, we just pause the tests for a couple of seconds to allow visual
/// inspection of the output.
fn check_screenshot(image: Handle, bt: &BootServices, name: &str) {
    if cfg!(feature = "qemu") {
        let serial_handles = bt
            .find_handles::<Serial>()
            .expect_success("Failed to get serial handles");

        // Use the second serial device handle. Opening a serial device
        // in exclusive mode breaks the connection between stdout and
        // the serial device, and we don't want that to happen to the
        // first serial device since it's used for log transport.
        let serial_handle = *serial_handles
            .get(1)
            .expect("Second serial device is missing");

        let serial = bt
            .open_protocol::<Serial>(
                OpenProtocolParams {
                    handle: serial_handle,
                    agent: image,
                    controller: None,
                },
                OpenProtocolAttributes::Exclusive,
            )
            .expect_success("Could not open serial protocol");
        let serial = unsafe { &mut *serial.interface.get() };

        // Set a large timeout to avoid problems with Travis
        let mut io_mode = *serial.io_mode();
        io_mode.timeout = 10_000_000;
        serial
            .set_attributes(&io_mode)
            .expect_success("Failed to configure serial port timeout");

        // Send a screenshot request to the host
        serial
            .write(b"SCREENSHOT: ")
            .expect_success("Failed to send request");
        let name_bytes = name.as_bytes();
        serial
            .write(name_bytes)
            .expect_success("Failed to send request");
        serial.write(b"\n").expect_success("Failed to send request");

        // Wait for the host's acknowledgement before moving forward
        let mut reply = [0; 3];
        serial
            .read(&mut reply[..])
            .expect_success("Failed to read host reply");

        assert_eq!(&reply[..], b"OK\n", "Unexpected screenshot request reply");
    } else {
        // Outside of QEMU, give the user some time to inspect the output
        bt.stall(3_000_000);
    }
}

fn shutdown(image: uefi::Handle, mut st: SystemTable<Boot>) -> ! {
    use uefi::table::runtime::ResetType;

    // Get our text output back.
    st.stdout().reset(false).unwrap_success();

    // Inform the user, and give him time to read on real hardware
    if cfg!(not(feature = "qemu")) {
        info!("Testing complete, shutting down in 3 seconds...");
        st.boot_services().stall(3_000_000);
    } else {
        info!("Testing complete, shutting down...");
    }

    // Exit boot services as a proof that it works :)
    let sizes = st.boot_services().memory_map_size();
    let max_mmap_size = sizes.map_size + 2 * sizes.entry_size;
    let mut mmap_storage = vec![0; max_mmap_size].into_boxed_slice();
    let (st, _iter) = st
        .exit_boot_services(image, &mut mmap_storage[..])
        .expect_success("Failed to exit boot services");

    #[cfg(target_arch = "x86_64")]
    {
        if cfg!(feature = "qemu") {
            use qemu_exit::QEMUExit;
            let custom_exit_success = 3;
            let qemu_exit_handle = qemu_exit::X86::new(0xF4, custom_exit_success);
            qemu_exit_handle.exit_success();
        }
    }

    // Shut down the system
    let rt = unsafe { st.runtime_services() };
    rt.reset(ResetType::Shutdown, Status::SUCCESS, None);
}
