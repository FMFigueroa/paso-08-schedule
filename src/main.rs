// ─── Paso 8: Schedule & Auto Mode — El device sigue una curva horaria ───
//
// El modo Auto deja de ser respiración decorativa y pasa a aplicar la
// interpolación del scheduler según la hora local. Si el scheduler no
// tiene puntos cargados (o el reloj no sincronizó), fallback a una
// respiración suave para que el LED no quede apagado indefinidamente.

mod led;
mod light_schedule;
mod light_state;
mod provisioning;
mod secure_storage;
mod telemetry;
mod time_sync;
mod wifi;
mod ws_client;

use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;

#[allow(unused_imports)]
use esp_idf_svc::sys as _;

use log::{error, info, warn};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use led::LedController;
use light_schedule::LightScheduler;
use light_state::{LightState, Mode};
use secure_storage::SecureStorage;
use telemetry::TelemetryReport;
use ws_client::{OutgoingMessage, WsClient};

const FALLBACK_BRIGHTNESS_STEPS: &[u8] = &[0, 25, 50, 75, 100, 75, 50, 25];
const LOOP_TICK_MS: u32 = 500;
const TELEMETRY_INTERVAL: Duration = Duration::from_secs(60);

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    info!("paso-08-schedule");

    if let Err(e) = run() {
        error!("Error fatal: {:?}", e);
        std::thread::sleep(Duration::from_secs(10));
        unsafe {
            esp_idf_svc::sys::esp_restart();
        }
    }
}

fn run() -> anyhow::Result<()> {
    let boot_time = Instant::now();

    let peripherals = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs_partition = EspDefaultNvsPartition::take()?;

    let led_controller = LedController::new(peripherals.rmt.channel0, peripherals.pins.gpio2)?;
    let led = Arc::new(Mutex::new(led_controller));

    let storage = SecureStorage::new(nvs_partition.clone())?;
    let storage = Arc::new(Mutex::new(storage));

    let is_provisioned = { storage.lock().unwrap().is_provisioned()? };
    if !is_provisioned {
        warn!("Device not provisioned!");
        provisioning::start_provisioning(peripherals.modem, sysloop, storage)?;
        return Ok(());
    }

    let credentials = { storage.lock().unwrap().load_credentials()? };
    let device_id = credentials.device_id.clone();

    info!("Connecting to WiFi: {}", credentials.wifi_ssid);
    let _wifi = wifi::connect(
        &credentials.wifi_ssid,
        &credentials.wifi_password,
        peripherals.modem,
        sysloop,
    )?;
    drop(credentials);

    let _sntp = time_sync::init_ntp()?;

    // ─── NUEVO EN PASO 8 ───
    //
    // LightScheduler compartido entre main (lee) y WS thread (escribe).
    // Mismo patrón Arc<Mutex<>> que el LightState.
    let scheduler = Arc::new(Mutex::new(LightScheduler::default()));
    let light_state = Arc::new(Mutex::new(LightState::default()));

    let ws = WsClient::new(light_state.clone(), scheduler.clone())?;
    ws.send(OutgoingMessage::Hello {
        device_id: device_id.clone(),
    })?;

    info!("Entering main loop — Auto mode uses schedule if available");

    let mut fallback_idx: usize = 0;
    let mut last_manual_intensity: u8 = 255;
    let mut next_telemetry = Instant::now() + TELEMETRY_INTERVAL;

    loop {
        let snapshot = { *light_state.lock().unwrap() };

        match snapshot.mode {
            Mode::Auto => {
                // Intentamos aplicar el schedule. Si no hay schedule o no hay
                // hora sincronizada, fallback a respiración del paso anterior.
                let schedule_result = time_sync::get_current_hm()
                    .and_then(|(h, m)| scheduler.lock().ok().and_then(|s| s.evaluate(h, m)));

                let (intensity, temperature) = match schedule_result {
                    Some((i, t)) => (i, t),
                    None => {
                        // Fallback: respiración del paso-03
                        let i = FALLBACK_BRIGHTNESS_STEPS[fallback_idx];
                        fallback_idx = (fallback_idx + 1) % FALLBACK_BRIGHTNESS_STEPS.len();
                        (i, snapshot.temperature)
                    }
                };

                led.lock().unwrap().set_brightness(intensity)?;
                {
                    let mut s = light_state.lock().unwrap();
                    s.intensity = intensity;
                    s.temperature = temperature;
                }
                last_manual_intensity = 255;
            }
            Mode::Manual => {
                if snapshot.intensity != last_manual_intensity {
                    led.lock().unwrap().set_brightness(snapshot.intensity)?;
                    last_manual_intensity = snapshot.intensity;
                }
            }
        }

        if Instant::now() >= next_telemetry {
            let mode_str = match snapshot.mode {
                Mode::Auto => "auto",
                Mode::Manual => "manual",
            };
            let report = TelemetryReport::new(boot_time)
                .with_heap()
                .with_light_state(snapshot.intensity, mode_str);

            if let Some(s) = time_sync::get_local_time_string() {
                info!("[{}] Telemetry: heap={:?}", s, report.heap_free_bytes);
            }

            if let Err(e) = ws.send(OutgoingMessage::Telemetry(report)) {
                warn!("Failed to enqueue telemetry: {:?}", e);
            }
            next_telemetry = Instant::now() + TELEMETRY_INTERVAL;
        }

        FreeRtos::delay_ms(LOOP_TICK_MS);
    }
}
