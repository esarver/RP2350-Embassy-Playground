#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use cortex_m::prelude::_embedded_hal_blocking_delay_DelayMs;
use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::{
    bind_interrupts,
    gpio::{Input, Level, Output, Pull},
    peripherals::USB,
    usb::{Driver, InterruptHandler},
};
use embassy_time::{Delay, Timer};

use embassy_usb::{
    Builder, Config, Handler,
    class::hid::{HidReaderWriter, HidWriter, RequestHandler, State},
    control::OutResponse,
    driver::EndpointError,
};
// Ensure we halt the program on panic (if we don't mention this crate it won't
// be linked)
use panic_halt as _;

use defmt_rtt as _;
use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    //Create the driver, using the HAL
    let driver = Driver::new(p.USB, Irqs);

    //Configure USB HID for a keyboard
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Edwin");
    config.product = Some("Edwin Typer 2000");
    config.serial_number = Some("00000001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // Create Device Builder using driver and config.
    // It needs some buffers for building the descriptors
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 64];
    let mut request_handler = Rh {};
    let mut device_handler = Dh::new();

    let mut state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    builder.handler(&mut device_handler);

    let config = embassy_usb::class::hid::Config {
        report_descriptor: KeyboardReport::desc(),
        request_handler: None,
        poll_ms: 60,
        max_packet_size: 64,
    };

    let hid = HidReaderWriter::<_, 1, 8>::new(&mut builder, &mut state, config);

    let mut usb = builder.build();
    let usb_fut = usb.run();

    let mut led = Output::new(p.PIN_25, Level::Low);
    let mut button = Input::new(p.PIN_16, Pull::None);
    button.set_schmitt(true); // debounce

    let (reader, mut writer) = hid.split();

    // After the code is done, blink the LED.
    let in_fut = async {
        loop {
            info!("Waiting for button...");
            button.wait_for_high().await;
            info!("Pressed!");
            led.set_high();

            // META + R for windows run menu
            let report = KeyboardReport {
                modifier: 0x08,
                reserved: 0,
                leds: 0,
                keycodes: [0x15, 0, 0, 0, 0, 0],
            };
            match writer.write_serialize(&report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report {:?}", e),
            };

            Timer::after_millis(600).await;

            // CTRL + A select all
            let report = KeyboardReport {
                modifier: 0x01,
                reserved: 0,
                leds: 0,
                keycodes: [0x04, 0, 0, 0, 0, 0],
            };
            match writer.write_serialize(&report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report {:?}", e),
            };
            Timer::after_millis(500).await;

            match type_to_computer(&mut writer, b"notepad.exe\n").await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report {:?}", e),
            };
            Timer::after_millis(500).await;

            match type_to_computer(&mut writer, b"Edwin was here!").await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report {:?}", e),
            };
            Timer::after_millis(900).await;

            // CTRL + S save
            let report = KeyboardReport {
                modifier: 0x01,
                reserved: 0,
                leds: 0,
                keycodes: [0x16, 0, 0, 0, 0, 0],
            };
            match writer.write_serialize(&report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report {:?}", e),
            };
            Timer::after_millis(500).await;

            match type_to_computer(&mut writer, b"EdwinWasHere.txt\n").await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report {:?}", e),
            };
            Timer::after_millis(700).await;

            // Alt + F4 Close Notepad
            let report = KeyboardReport {
                modifier: 0x04,
                reserved: 0,
                leds: 0,
                keycodes: [0x3D, 0, 0, 0, 0, 0],
            };
            match writer.write_serialize(&report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report {:?}", e),
            };
            Timer::after_millis(500).await;

            let report = KeyboardReport {
                modifier: 0,
                reserved: 0,
                leds: 0,
                keycodes: [0, 0, 0, 0, 0, 0],
            };
            match writer.write_serialize(&report).await {
                Ok(()) => {}
                Err(e) => warn!("Failed to send report {:?}", e),
            };

            led.set_low();

            button.wait_for_low().await;
            info!("Unpressed!");
            info!("Done!");
        }
    };

    let out_fut = async {
        reader.run(false, &mut request_handler).await;
    };

    join(usb_fut, join(in_fut, out_fut)).await;
}

async fn type_to_computer<'a>(
    writer: &mut HidWriter<'a, Driver<'a, USB>, 8>,
    buf: &[u8],
) -> Result<(), EndpointError> {
    for b in buf {
        let r = get_kb_report(*b);
        if r.is_none() {
            break;
        }
        let r = r.unwrap();
        writer.write_serialize(&r).await?;
        let report = KeyboardReport {
            modifier: 0,
            reserved: 0,
            leds: 0,
            keycodes: [0, 0, 0, 0, 0, 0],
        };
        match writer.write_serialize(&report).await {
            Ok(()) => {}
            Err(e) => warn!("Failed to send report {:?}", e),
        };
        Timer::after_millis(50).await;
    }
    Ok(())
}

const fn get_kb_report(c: u8) -> Option<KeyboardReport> {
    match c {
        b'a'..=b'z' => Some(KeyboardReport {
            modifier: 0,
            reserved: 0,
            leds: 0,
            keycodes: [c - 93, 0, 0, 0, 0, 0],
        }),
        b'A'..=b'Z' => Some(KeyboardReport {
            modifier: 2, // Left Shift
            reserved: 0,
            leds: 0,
            keycodes: [c - 61, 0, 0, 0, 0, 0],
        }),
        b'1'..=b'9' => Some(KeyboardReport {
            modifier: 0,
            reserved: 0,
            leds: 0,
            keycodes: [c - 19, 0, 0, 0, 0, 0],
        }),
        b'0' => Some(KeyboardReport {
            modifier: 0,
            reserved: 0,
            leds: 0,
            keycodes: [0x27, 0, 0, 0, 0, 0],
        }),
        b'.' => Some(KeyboardReport {
            modifier: 0,
            reserved: 0,
            leds: 0,
            keycodes: [0x37, 0, 0, 0, 0, 0],
        }),
        b'!' => Some(KeyboardReport {
            modifier: 2, // Left Shift
            reserved: 0,
            leds: 0,
            keycodes: [0x1e, 0, 0, 0, 0, 0],
        }),
        b'\n' => Some(KeyboardReport {
            modifier: 0, // Left Shift
            reserved: 0,
            leds: 0,
            keycodes: [0x28, 0, 0, 0, 0, 0],
        }),
        b' ' => Some(KeyboardReport {
            modifier: 0, // Left Shift
            reserved: 0,
            leds: 0,
            keycodes: [0x2c, 0, 0, 0, 0, 0],
        }),
        _ => None,
    }
}

struct Rh {}

impl RequestHandler for Rh {
    fn get_report(
        &mut self,
        id: embassy_usb::class::hid::ReportId,
        _buf: &mut [u8],
    ) -> Option<usize> {
        info!("Get report for {:?}", id);
        None
    }

    fn set_report(&mut self, id: embassy_usb::class::hid::ReportId, data: &[u8]) -> OutResponse {
        info!("Set report for {:?}: {=[u8]}", id, data);
        OutResponse::Accepted
    }

    fn get_idle_ms(&mut self, id: Option<embassy_usb::class::hid::ReportId>) -> Option<u32> {
        info!("Get idle rate for {:?}", id);
        None
    }

    fn set_idle_ms(&mut self, id: Option<embassy_usb::class::hid::ReportId>, duration_ms: u32) {
        info!("Set idle rate for {:?} to {:?}", id, duration_ms);
    }
}

struct Dh {
    configured: AtomicBool,
}

impl Dh {
    fn new() -> Self {
        Self {
            configured: AtomicBool::new(false),
        }
    }
}

impl Handler for Dh {
    fn enabled(&mut self, enabled: bool) {
        self.configured.store(false, Ordering::Relaxed);
        if enabled {
            info!("Device enabled");
        } else {
            info!("Device disabled");
        }
    }

    fn reset(&mut self) {
        self.configured.store(false, Ordering::Relaxed);
        info!("Bus reset, the Vbus current limit is 100mA");
    }

    fn addressed(&mut self, addr: u8) {
        self.configured.store(false, Ordering::Relaxed);
        info!("USB address set to: {}", addr);
    }

    fn configured(&mut self, configured: bool) {
        self.configured.store(configured, Ordering::Relaxed);
        if configured {
            info!(
                "Device is configured, it may now draw up to the configured current limit from Vbus"
            );
        } else {
            info!("Device is no longer configured, the Vbus current limit is 100mA.");
        }
    }
}
