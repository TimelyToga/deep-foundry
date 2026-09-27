//! Number formats, in the Factorio style.
//!
//! - Counts in slots: `999`, `1.2k`, `45k`, `1.2M`. The value is rounded down, so the
//!   slot never shows more than the player has.
//! - Power: `450 W`, `1.2 kW`, `36 kW`, `1.8 MW`.
//! - Energy: `500 J`, `1.2 kJ`, `4.5 MJ`.

/// A count for a slot corner. Rounded down. Examples: 999, 1k, 1.2k, 45k, 1.2M, 45M, 1.2G.
pub fn count(n: u64) -> String {
    const UNITS: [(u64, &str); 3] = [(1_000_000_000, "G"), (1_000_000, "M"), (1_000, "k")];
    for (size, suffix) in UNITS {
        if n >= size {
            let whole = n / size;
            if whole < 10 {
                let tenth = (n % size) * 10 / size;
                return if tenth == 0 { format!("{whole}{suffix}") } else { format!("{whole}.{tenth}{suffix}") };
            }
            return format!("{whole}{suffix}");
        }
    }
    n.to_string()
}

/// A number with a unit and an SI prefix (k, M, G, T). Below 10 it shows one decimal.
/// Examples: `si(450.0, "W")` = "450 W", `si(1234.0, "W")` = "1.2 kW", `si(36000.0, "W")` = "36 kW".
pub fn si(value: f64, unit: &str) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    let v = value.abs();
    const PREFIXES: [(f64, &str); 5] = [(1e12, "T"), (1e9, "G"), (1e6, "M"), (1e3, "k"), (1.0, "")];
    for (size, prefix) in PREFIXES {
        if v >= size {
            let scaled = v / size;
            return if scaled < 10.0 && size > 1.0 {
                let text = format!("{:.1}", (scaled * 10.0).floor() / 10.0);
                let text = text.strip_suffix(".0").unwrap_or(&text).to_string();
                format!("{sign}{text} {prefix}{unit}")
            } else {
                format!("{sign}{} {prefix}{unit}", scaled.floor() as u64)
            };
        }
    }
    if v == 0.0 { format!("0 {unit}") } else { format!("{sign}{:.1} {unit}", v) }
}

/// Power in watts: "450 W", "1.2 kW", "36 kW".
pub fn watts(w: f64) -> String {
    si(w, "W")
}

/// Energy in joules: "500 J", "4.5 MJ".
pub fn joules(j: f64) -> String {
    si(j, "J")
}

/// Temperature, rounded to whole degrees: "1085 °C".
pub fn celsius(c: f32) -> String {
    format!("{} °C", c.round() as i64)
}

/// A crafting time: "0.5 s", "5 s", "1 m 30 s".
pub fn seconds(s: f32) -> String {
    if s < 10.0 {
        let text = format!("{:.1}", s);
        let text = text.strip_suffix(".0").unwrap_or(&text).to_string();
        format!("{text} s")
    } else if s < 60.0 {
        format!("{} s", s.round() as u32)
    } else {
        let total = s.round() as u32;
        let (m, sec) = (total / 60, total % 60);
        if sec == 0 { format!("{m} m") } else { format!("{m} m {sec} s") }
    }
}

/// Play time: "12 m", "3 h 05 m".
pub fn play_time(total_s: u64) -> String {
    let h = total_s / 3600;
    let m = (total_s % 3600) / 60;
    if h == 0 { format!("{m} m") } else { format!("{h} h {m:02} m") }
}

/// A rate per minute: "12/min", "1.2k/min", "0.5/min".
pub fn per_minute(rate: f32) -> String {
    if rate <= 0.0 {
        "0/min".to_string()
    } else if rate < 10.0 {
        let text = format!("{:.1}", rate);
        let text = text.strip_suffix(".0").unwrap_or(&text).to_string();
        format!("{text}/min")
    } else {
        format!("{}/min", count(rate as u64))
    }
}

/// A fraction as a percent: 0.853 -> "85%".
pub fn percent(frac: f32) -> String {
    format!("{}%", (frac * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_use_factorio_style() {
        assert_eq!(count(0), "0");
        assert_eq!(count(7), "7");
        assert_eq!(count(999), "999");
        assert_eq!(count(1000), "1k");
        assert_eq!(count(1050), "1k");
        assert_eq!(count(1200), "1.2k");
        assert_eq!(count(1299), "1.2k");
        assert_eq!(count(9999), "9.9k");
        assert_eq!(count(10_000), "10k");
        assert_eq!(count(45_000), "45k");
        assert_eq!(count(45_999), "45k");
        assert_eq!(count(999_999), "999k");
        assert_eq!(count(1_000_000), "1M");
        assert_eq!(count(1_234_567), "1.2M");
        assert_eq!(count(45_000_000), "45M");
        assert_eq!(count(1_500_000_000), "1.5G");
    }

    #[test]
    fn power_and_energy() {
        assert_eq!(watts(0.0), "0 W");
        assert_eq!(watts(450.0), "450 W");
        assert_eq!(watts(1000.0), "1 kW");
        assert_eq!(watts(1234.0), "1.2 kW");
        assert_eq!(watts(36_000.0), "36 kW");
        assert_eq!(watts(450_000.0), "450 kW");
        assert_eq!(watts(1_800_000.0), "1.8 MW");
        assert_eq!(watts(-2500.0), "-2.5 kW");
        assert_eq!(joules(4_500_000.0), "4.5 MJ");
    }

    #[test]
    fn times_and_rates() {
        assert_eq!(seconds(0.5), "0.5 s");
        assert_eq!(seconds(5.0), "5 s");
        assert_eq!(seconds(20.0), "20 s");
        assert_eq!(seconds(60.0), "1 m");
        assert_eq!(seconds(90.0), "1 m 30 s");
        assert_eq!(play_time(59), "0 m");
        assert_eq!(play_time(12 * 60), "12 m");
        assert_eq!(play_time(3 * 3600 + 5 * 60), "3 h 05 m");
        assert_eq!(per_minute(0.0), "0/min");
        assert_eq!(per_minute(0.5), "0.5/min");
        assert_eq!(per_minute(12.0), "12/min");
        assert_eq!(per_minute(1500.0), "1.5k/min");
        assert_eq!(celsius(1084.6), "1085 °C");
        assert_eq!(percent(0.853), "85%");
    }
}
