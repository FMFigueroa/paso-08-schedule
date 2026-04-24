// ─── Paso 5: Módulo Light State — Estado global de la luz ───
//
// Hasta paso-04 el firmware tenía "estado" disperso: un LED con su
// brillo actual, un Option<Instant> para el override, ninguna noción de
// "modo del dispositivo". Esto no escala — cuando introduzcamos schedule,
// telemetría y comandos de alto nivel, cada feature tendría que
// inventar su propio estado.
//
// Este módulo define un struct `LightState` compartido por todo el
// firmware, con el mínimo necesario para modelar una lámpara:
// - intensity: 0..100 (lo que era set_brightness)
// - temperature: 0..100 (0 = cálido, 100 = frío — el LED blanco discreto
//   no lo usa, pero queda sembrado para cuando aparezca el WS2812)
// - mode: Manual | Auto (qué controla el brillo en cada momento)
//
// El main loop consulta `mode`:
// - Auto → ejecuta la respiración y actualiza `intensity` mientras avanza
// - Manual → aplica `intensity` al LED y no interfiere
//
// El WS modifica `intensity`, `temperature`, `mode` según los comandos
// recibidos (`SetLight`, `SetMode`). Todo pasa por este struct.

use serde::{Deserialize, Serialize};

// ─── Enum Mode ───

/// Quién manda en el LED.
///
/// `Auto`: el firmware corre la respiración solo (default al boot).
/// `Manual`: el LED queda fijo en `intensity` hasta nueva orden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Manual,
    Auto,
}

impl Default for Mode {
    fn default() -> Self {
        Self::Auto
    }
}

// ─── Struct LightState ───

/// Estado de la luz compartido por todo el firmware.
///
/// Tres campos, todos primitivos:
/// - `intensity` ∈ [0, 100]: brillo percibido
/// - `temperature` ∈ [0, 100]: 0=cálido (2700K), 100=frío (6500K)
/// - `mode`: Manual | Auto
///
/// Deriva `Serialize` + `Deserialize` para reusarse directamente como
/// payload de los mensajes WS (`LightState` outgoing, `SetLight` por
/// campos individuales).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LightState {
    pub intensity: u8,
    pub temperature: u8,
    pub mode: Mode,
}

impl Default for LightState {
    /// Estado inicial: respirando en modo Auto, intensidad 0 (arranca apagado
    /// y la respiración lo prende suavemente), temperatura neutra.
    fn default() -> Self {
        Self {
            intensity: 0,
            temperature: 50,
            mode: Mode::Auto,
        }
    }
}

impl LightState {
    /// Aplica un update parcial: si `Some(v)` → actualiza; si `None` → deja.
    ///
    /// El WS manda `SetLight { intensity: Option<u8>, temperature: Option<u8> }`.
    /// Esta función centraliza la lógica de "actualizar solo lo que vino".
    /// También **cambia el modo a Manual** si cualquiera de los dos campos se
    /// modificó — es la convención: si vos mandás un valor concreto, tomás
    /// el control. Para volver a Auto, tenés que mandar SetMode explícito.
    pub fn apply_set_light(&mut self, intensity: Option<u8>, temperature: Option<u8>) {
        let mut touched = false;

        if let Some(i) = intensity {
            self.intensity = i.min(100);
            touched = true;
        }
        if let Some(t) = temperature {
            self.temperature = t.min(100);
            touched = true;
        }

        if touched {
            self.mode = Mode::Manual;
        }
    }
}
