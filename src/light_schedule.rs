// ─── Paso 8: Módulo Light Schedule — Curva horaria con interpolación lineal ───
//
// El modo Auto del paso-05 hasta ahora era "respirar" (un ciclo
// decorativo). Acá lo reemplazamos por algo con sentido de dominio:
// el backend manda un schedule (lista de puntos horarios) y el device
// interpola la intensity/temperature entre los puntos según la hora
// actual.
//
// Ejemplo de schedule:
//   06:00 → { intensity: 40, temperature: 30 }  (amanecer, cálido)
//   12:00 → { intensity: 90, temperature: 60 }  (mediodía, neutro)
//   22:00 → { intensity: 15, temperature: 80 }  (noche, frío bajo)
//
// A las 09:00 el scheduler calcula: estamos entre 06:00 y 12:00.
// Fracción = (09-06) / (12-06) = 0.5
// intensity = lerp(40, 90, 0.5) = 65
// temperature = lerp(30, 60, 0.5) = 45
//
// El dominio es **circular 24h**: el intervalo entre 22:00 y 06:00 del
// día siguiente se atraviesa la medianoche. Esto requiere un poco de
// aritmética modular 24h.

use serde::{Deserialize, Serialize};

// ─── SchedulePoint ───

/// Un punto del schedule.
///
/// `hour` en [0, 23], `minute` en [0, 59].
/// `intensity` y `temperature` en [0, 100].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SchedulePoint {
    pub hour: u8,
    pub minute: u8,
    pub intensity: u8,
    pub temperature: u8,
}

impl SchedulePoint {
    /// Tiempo del punto en minutos desde medianoche (0..1440).
    /// Facilita comparaciones y la interpolación.
    fn minutes_of_day(&self) -> u16 {
        (self.hour as u16) * 60 + (self.minute as u16)
    }
}

// ─── LightScheduler ───

/// Scheduler con lista ordenada de puntos.
///
/// Uso:
///   let mut s = LightScheduler::default();
///   s.set_schedule(vec![...]);  // recibido del backend
///   if s.has_schedule() {
///     let (i, t) = s.evaluate(hour, minute)?;
///     ...
///   }
pub struct LightScheduler {
    points: Vec<SchedulePoint>,
}

impl Default for LightScheduler {
    fn default() -> Self {
        Self { points: Vec::new() }
    }
}

impl LightScheduler {
    /// Reemplaza el schedule entero. Los puntos se ordenan por hora antes
    /// de guardarse — para evitar que el caller se preocupe por el orden.
    pub fn set_schedule(&mut self, mut points: Vec<SchedulePoint>) {
        points.sort_by_key(|p| p.minutes_of_day());
        self.points = points;
    }

    pub fn has_schedule(&self) -> bool {
        !self.points.is_empty()
    }

    /// Evalúa el schedule en la hora dada. Retorna `Some((intensity, temp))`
    /// si hay al menos un punto cargado.
    ///
    /// Algoritmo:
    /// 1. Encontrar el segmento (prev, next) que contiene `now`.
    /// 2. Si `now` está antes del primer punto o después del último, el
    ///    segmento atraviesa medianoche (wrap-around).
    /// 3. Interpolar linealmente entre prev y next según fracción.
    pub fn evaluate(&self, hour: u8, minute: u8) -> Option<(u8, u8)> {
        if self.points.is_empty() {
            return None;
        }

        let now = (hour as u16) * 60 + (minute as u16);

        // Encontrar el primer punto cuyo tiempo sea >= now.
        // Si todos son < now, el segmento va del último al primero+24h.
        let (prev, next, next_time) =
            match self.points.iter().position(|p| p.minutes_of_day() >= now) {
                Some(0) => {
                    // now está antes del primer punto → segmento es (último, primero)
                    // con wrap-around: next_time = primero.time() + 1440
                    let prev = *self.points.last().unwrap();
                    let next = self.points[0];
                    (prev, next, next.minutes_of_day() as u32 + 1440)
                }
                Some(idx) => {
                    // segmento normal dentro del día
                    let prev = self.points[idx - 1];
                    let next = self.points[idx];
                    (prev, next, next.minutes_of_day() as u32)
                }
                None => {
                    // now está después del último punto → segmento es (último, primero+24h)
                    let prev = *self.points.last().unwrap();
                    let next = self.points[0];
                    (prev, next, next.minutes_of_day() as u32 + 1440)
                }
            };

        // Ajuste del "now" en el mismo espacio que next_time (maybe +1440 si wrap).
        // Si prev_time > next_time_adjusted... no puede pasar porque ya normalizamos.
        let prev_time = prev.minutes_of_day() as u32;
        let now_adjusted = if now as u32 >= prev_time {
            now as u32
        } else {
            // now está en el segmento post-medianoche del wrap-around
            now as u32 + 1440
        };

        let total = next_time - prev_time;
        let elapsed = now_adjusted - prev_time;

        // Protegerse contra total == 0 (dos puntos en el mismo minuto)
        if total == 0 {
            return Some((next.intensity, next.temperature));
        }

        let intensity = lerp_u8(prev.intensity, next.intensity, elapsed, total);
        let temperature = lerp_u8(prev.temperature, next.temperature, elapsed, total);
        Some((intensity, temperature))
    }
}

// ─── Helpers ───

/// Interpolación lineal en u8 con aritmética entera.
///
/// `lerp_u8(a, b, num, den)` devuelve aproximadamente `a + (b-a) * num/den`,
/// manejando bien el caso `b < a` (sin overflow de u8).
fn lerp_u8(a: u8, b: u8, num: u32, den: u32) -> u8 {
    if a <= b {
        let delta = (b - a) as u32;
        (a as u32 + delta * num / den) as u8
    } else {
        let delta = (a - b) as u32;
        (a as u32 - delta * num / den) as u8
    }
}
