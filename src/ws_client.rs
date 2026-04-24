// ─── Paso 8: Módulo WebSocket Client — FFI + SyncSchedule ───
//
// Extiende el WS client de paso-05: agrega variant Telemetry(TelemetryReport)
// al enum OutgoingMessage. Cero cambios en el flujo — el enum es extensible.

use anyhow::{anyhow, Result};
use esp_idf_svc::sys::*;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::light_schedule::{LightScheduler, SchedulePoint};
use crate::light_state::{LightState, Mode};
use crate::telemetry::TelemetryReport;

// ─── Conexión ───

const WS_HOST: &str = "ws.postman-echo.com";
const WS_PORT: i32 = 443;
const WS_PATH: &str = "/raw";
const CONNECT_TIMEOUT_MS: i32 = 10_000;
const READ_TIMEOUT_MS: i32 = 100;
const RECONNECT_DELAY_SECS: u64 = 5;
const WS_THREAD_STACK_BYTES: usize = 16 * 1024;

// ─── Opcodes WS ───

const WS_OPCODE_FIN: u8 = 0x80;
const WS_OPCODE_TEXT: u8 = 0x01;
const WS_OPCODE_CLOSE: u8 = 0x08;
const WS_OPCODE_PING: u8 = 0x09;
const WS_OPCODE_PONG: u8 = 0x0A;
const WS_OPCODE_TEXT_FIN: u8 = WS_OPCODE_TEXT | WS_OPCODE_FIN;

// ─── Schema ───

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum OutgoingMessage {
    Hello {
        device_id: String,
    },
    LightState(LightState),
    Ack {
        command: String,
    },
    /// NUEVA EN PASO 6: reporte periódico de salud.
    Telemetry(TelemetryReport),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum IncomingMessage {
    Hello {
        device_id: String,
    },
    SetLight {
        intensity: Option<u8>,
        temperature: Option<u8>,
    },
    SetMode {
        mode: Mode,
    },
    RequestState,
    /// NUEVA EN PASO 8: el backend sincroniza la curva horaria.
    SyncSchedule {
        points: Vec<SchedulePoint>,
    },
}

// ─── Struct público ───

pub struct WsClient {
    outbound_tx: Sender<OutgoingMessage>,
}

impl WsClient {
    pub fn new(
        light_state: Arc<Mutex<LightState>>,
        scheduler: Arc<Mutex<LightScheduler>>,
    ) -> Result<Self> {
        let (outbound_tx, outbound_rx) = mpsc::channel::<OutgoingMessage>();
        // Clone del sender para que el thread WS pueda generar ACKs + LightState en respuesta.
        let self_tx = outbound_tx.clone();

        thread::Builder::new()
            .name("ws_client".into())
            .stack_size(WS_THREAD_STACK_BYTES)
            .spawn(move || {
                ws_connection_loop(outbound_rx, light_state, scheduler, self_tx);
            })?;

        info!("WS client thread spawned");
        Ok(Self { outbound_tx })
    }

    pub fn send(&self, msg: OutgoingMessage) -> Result<()> {
        self.outbound_tx
            .send(msg)
            .map_err(|e| anyhow!("ws send failed: {}", e))
    }
}

// ─── Loop con reconexión ───

fn ws_connection_loop(
    outbound_rx: Receiver<OutgoingMessage>,
    light_state: Arc<Mutex<LightState>>,
    scheduler: Arc<Mutex<LightScheduler>>,
    self_tx: Sender<OutgoingMessage>,
) {
    loop {
        info!("WS: connecting to wss://{}{}", WS_HOST, WS_PATH);

        match connect_and_run(&outbound_rx, &light_state, &scheduler, &self_tx) {
            Ok(_) => info!("WS: disconnected normally"),
            Err(e) => error!("WS error: {:?}", e),
        }

        info!("WS: reconnecting in {}s...", RECONNECT_DELAY_SECS);
        thread::sleep(Duration::from_secs(RECONNECT_DELAY_SECS));
    }
}

fn connect_and_run(
    outbound_rx: &Receiver<OutgoingMessage>,
    light_state: &Arc<Mutex<LightState>>,
    scheduler: &Arc<Mutex<LightScheduler>>,
    self_tx: &Sender<OutgoingMessage>,
) -> Result<()> {
    let ssl = unsafe { esp_transport_ssl_init() };
    if ssl.is_null() {
        return Err(anyhow!("Failed to create SSL transport"));
    }
    unsafe {
        esp_transport_ssl_enable_global_ca_store(ssl);
        esp_transport_ssl_crt_bundle_attach(ssl, Some(esp_crt_bundle_attach));
    }

    let ws = unsafe { esp_transport_ws_init(ssl) };
    if ws.is_null() {
        unsafe {
            esp_transport_destroy(ssl);
        }
        return Err(anyhow!("Failed to create WS transport"));
    }

    let path = CString::new(WS_PATH)?;
    unsafe {
        esp_transport_ws_set_path(ws, path.as_ptr());
    }

    let host = CString::new(WS_HOST)?;
    let ret = unsafe { esp_transport_connect(ws, host.as_ptr(), WS_PORT, CONNECT_TIMEOUT_MS) };
    if ret != 0 {
        unsafe {
            esp_transport_close(ws);
            esp_transport_destroy(ws);
        }
        return Err(anyhow!("esp_transport_connect failed: {}", ret));
    }

    info!("WS connected!");
    let mut read_buf = [0u8; 4096];

    loop {
        let bytes_read = unsafe {
            esp_transport_read(
                ws,
                read_buf.as_mut_ptr() as *mut u8,
                read_buf.len() as i32,
                READ_TIMEOUT_MS,
            )
        };

        if bytes_read > 0 {
            let opcode = unsafe { esp_transport_ws_get_read_opcode(ws) as u8 };
            match opcode {
                WS_OPCODE_TEXT => {
                    if let Ok(text) = std::str::from_utf8(&read_buf[..bytes_read as usize]) {
                        handle_text_frame(text, light_state, scheduler, self_tx);
                    }
                }
                WS_OPCODE_PING => {
                    info!("WS: ping received");
                    unsafe {
                        esp_transport_ws_send_raw(
                            ws,
                            (WS_OPCODE_PONG | WS_OPCODE_FIN) as ws_transport_opcodes_t,
                            ptr::null(),
                            0,
                            CONNECT_TIMEOUT_MS,
                        );
                    }
                }
                WS_OPCODE_CLOSE => {
                    info!("WS: close frame received");
                    break;
                }
                _ => info!("WS: opcode 0x{:02X} ignored", opcode),
            }
        } else if bytes_read == 0 {
            info!("WS: peer closed");
            break;
        } else if bytes_read != -1 {
            error!("WS read error: {}", bytes_read);
            break;
        }

        // Drenar outgoing
        while let Ok(msg) = outbound_rx.try_recv() {
            if let Ok(json) = serde_json::to_string(&msg) {
                info!("WS → {}", json);
                let ret = unsafe {
                    esp_transport_ws_send_raw(
                        ws,
                        WS_OPCODE_TEXT_FIN as ws_transport_opcodes_t,
                        json.as_ptr() as *const u8,
                        json.len() as i32,
                        CONNECT_TIMEOUT_MS,
                    )
                };
                if ret < 0 {
                    warn!("WS send failed: {}", ret);
                }
            }
        }

        thread::sleep(Duration::from_millis(10));
    }

    unsafe {
        esp_transport_close(ws);
        esp_transport_destroy(ws);
    }
    Ok(())
}

fn handle_text_frame(
    text: &str,
    light_state: &Arc<Mutex<LightState>>,
    scheduler: &Arc<Mutex<LightScheduler>>,
    self_tx: &Sender<OutgoingMessage>,
) {
    info!("WS ← {}", text);
    let msg: IncomingMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("WS: parse error: {} — payload: {}", e, text);
            return;
        }
    };

    match msg {
        IncomingMessage::Hello { device_id } => {
            info!("WS ← Hello from: {}", device_id);
        }
        IncomingMessage::SetLight {
            intensity,
            temperature,
        } => {
            if let Ok(mut s) = light_state.lock() {
                s.apply_set_light(intensity, temperature);
                info!(
                    "WS ← SetLight applied: intensity={} temperature={} mode={:?}",
                    s.intensity, s.temperature, s.mode
                );
            }
            let _ = self_tx.send(OutgoingMessage::Ack {
                command: "SetLight".into(),
            });
        }
        IncomingMessage::SetMode { mode } => {
            if let Ok(mut s) = light_state.lock() {
                s.mode = mode;
                info!("WS ← SetMode: {:?}", s.mode);
            }
            let _ = self_tx.send(OutgoingMessage::Ack {
                command: "SetMode".into(),
            });
        }
        IncomingMessage::RequestState => {
            let snapshot = if let Ok(s) = light_state.lock() {
                *s
            } else {
                return;
            };
            let _ = self_tx.send(OutgoingMessage::LightState(snapshot));
        }
        IncomingMessage::SyncSchedule { points } => {
            let n = points.len();
            if let Ok(mut s) = scheduler.lock() {
                s.set_schedule(points);
                info!("WS ← SyncSchedule: {} points loaded", n);
            }
            let _ = self_tx.send(OutgoingMessage::Ack {
                command: "SyncSchedule".into(),
            });
        }
    }
}
