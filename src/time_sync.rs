// ─── Paso 7: Módulo Time Sync — SNTP + POSIX time ───
//
// El ESP32-C3 al arrancar no sabe qué hora es del mundo real. El reloj
// interno (RTC) arranca desde 0. Sin sincronización, `localtime()`
// devuelve algo como "1970-01-01 00:00:42" — inútil para schedules.
//
// Este módulo:
// 1. Inicializa el cliente SNTP contra pool.ntp.org (protocolo UDP simple)
// 2. Setea la zona horaria (POSIX TZ format, default "UTC+3" para Argentina)
// 3. Espera hasta que el reloj se actualice (máximo 10 segundos)
// 4. Expone helpers: get_current_hm() → (hora, minuto) y get_local_time_string()
//
// El struct EspSntp<'static> vive durante todo el firmware — si se dropea,
// el cliente SNTP para de sincronizarse y el reloj empieza a driftear.

use anyhow::{bail, Result};
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use esp_idf_svc::sys as raw;
use log::{info, warn};
use std::ffi::CString;
use std::time::Duration;

/// Zona horaria POSIX. Formato `<ABBR><OFFSET>` donde OFFSET se invierte
/// (UTC-3 se escribe "UTC+3" en POSIX — es la diferencia en sentido contrario).
const POSIX_TZ: &str = "UTC+3";

/// Timeout de sincronización inicial.
const SYNC_TIMEOUT: Duration = Duration::from_secs(10);

// ─── Public API ───

/// Inicializa SNTP y espera a que sincronice (o time out).
///
/// Retorna `Box<EspSntp<'static>>` — mantenelo vivo. Si se dropea, el
/// cliente deja de actualizarse.
pub fn init_ntp() -> Result<Box<EspSntp<'static>>> {
    info!("Initializing SNTP client...");

    // Setear zona horaria via POSIX setenv("TZ", ...) + tzset()
    set_timezone(POSIX_TZ)?;

    // EspSntp::new_default() usa servidores default del SDK (pool.ntp.org)
    let sntp = EspSntp::new_default()?;

    // Esperar hasta sincronización o timeout
    let start = std::time::Instant::now();
    loop {
        if sntp.get_sync_status() == SyncStatus::Completed {
            info!("SNTP synchronized");
            if let Some(s) = get_local_time_string() {
                info!("Current local time: {}", s);
            }
            return Ok(Box::new(sntp));
        }

        if start.elapsed() > SYNC_TIMEOUT {
            warn!(
                "SNTP sync timed out after {:?} — continuing anyway",
                SYNC_TIMEOUT
            );
            // Retornamos el client igual — va a seguir intentando en
            // background y eventualmente se sincroniza.
            return Ok(Box::new(sntp));
        }

        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Retorna (hora, minuto) del reloj local actual.
///
/// Si el reloj aún no fue sincronizado (o SNTP falló), retorna `None`.
/// Usado por el scheduler (paso-08) para interpolar la curva del día.
pub fn get_current_hm() -> Option<(u8, u8)> {
    let tm = read_localtime()?;
    Some((tm.tm_hour as u8, tm.tm_min as u8))
}

/// Retorna el timestamp local completo como "YYYY-MM-DD HH:MM:SS".
#[allow(dead_code)]
pub fn get_local_time_string() -> Option<String> {
    let tm = read_localtime()?;
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
    ))
}

// ─── Internals ───

/// Setea la variable de entorno TZ y llama tzset() para que localtime_r()
/// use la zona correcta.
fn set_timezone(tz: &str) -> Result<()> {
    let key = CString::new("TZ").unwrap();
    let val = CString::new(tz)?;

    // unsafe: setenv + tzset son de la libc estándar — siempre seguros de
    // llamar con punteros válidos (las CString garantizan NUL-terminated).
    unsafe {
        raw::setenv(key.as_ptr(), val.as_ptr(), 1);
        raw::tzset();
    }

    info!("Timezone set to {}", tz);
    Ok(())
}

/// Lee el struct `tm` del reloj local. Si el reloj no está sincronizado
/// (time_t < 2020-01-01), retorna None.
fn read_localtime() -> Option<raw::tm> {
    let mut now: raw::time_t = 0;
    let mut tm: raw::tm = unsafe { std::mem::zeroed() };

    unsafe {
        // time(NULL) devuelve los segundos desde epoch
        raw::time(&mut now);

        // Sanity check: si el tiempo es pre-2020, asumimos que SNTP no
        // sincronizó. 2020-01-01 UTC en epoch = 1577836800.
        if now < 1_577_836_800 {
            return None;
        }

        // localtime_r convierte time_t a struct tm aplicando la zona horaria
        // (que seteamos con tzset()). _r = reentrant (thread-safe).
        let result = raw::localtime_r(&now, &mut tm);
        if result.is_null() {
            return None;
        }
    }

    Some(tm)
}
