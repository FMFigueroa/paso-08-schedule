// ─── Paso 6: Módulo Telemetry — Reportes periódicos de salud ───
//
// Por primera vez el firmware habla SIN que nadie le pregunte. Cada 60
// segundos construye un TelemetryReport con métricas del sistema y lo
// manda via WS. El backend puede graficar evolución, alertar sobre
// memory leaks, monitorear uptime.
//
// El struct se construye con builder pattern — metódo fluído `.with_xxx()`
// que devuelve `self`. Si un campo falla al obtenerse (ej: RSSI cuando
// WiFi se cayó), lo dejamos `None` y serde lo omite del JSON con
// `#[serde(skip_serializing_if = "Option::is_none")]`.

use serde::Serialize;
use std::time::Instant;

/// Reporte periódico de salud del dispositivo.
///
/// Todos los campos fuera de `uptime_secs` son opcionales — si una métrica
/// no está disponible al momento del reporte, se omite del JSON para
/// ahorrar bytes y evitar enviar valores inválidos.
#[derive(Debug, Serialize)]
pub struct TelemetryReport {
    /// Segundos desde el boot. Siempre disponible.
    pub uptime_secs: u64,

    /// Heap libre en bytes. Consultado via FFI al SDK de C.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heap_free_bytes: Option<u32>,

    /// Intensidad de señal WiFi en dBm. Negativo (−40 es excelente,
    /// −90 muy pobre). En paso-06 lo dejamos como `None` porque acceder
    /// al RSSI desde un contexto sin la referencia directa al EspWifi
    /// es engorroso — lo habilitamos cuando migremos la arquitectura.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rssi_dbm: Option<i8>,

    /// Modo actual del LightState (para correlación). Siempre presente.
    pub mode: String,

    /// Intensidad actual del LED (para correlación). Siempre presente.
    pub intensity: u8,
}

impl TelemetryReport {
    /// Constructor base — solo el uptime (único campo obligatorio).
    pub fn new(boot_time: Instant) -> Self {
        Self {
            uptime_secs: Instant::now().duration_since(boot_time).as_secs(),
            heap_free_bytes: None,
            rssi_dbm: None,
            mode: String::new(),
            intensity: 0,
        }
    }

    /// Builder: agrega heap libre consultando el SDK de C.
    ///
    /// `esp_get_free_heap_size` es una función C exportada por ESP-IDF.
    /// La llamamos via FFI con `unsafe`. No tiene efectos colaterales —
    /// solo lee un contador interno del allocator.
    pub fn with_heap(mut self) -> Self {
        let free = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() };
        self.heap_free_bytes = Some(free);
        self
    }

    /// Builder: agrega el snapshot del LightState.
    pub fn with_light_state(mut self, intensity: u8, mode: &str) -> Self {
        self.intensity = intensity;
        self.mode = mode.into();
        self
    }
}
