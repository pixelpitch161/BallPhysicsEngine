//! Decibels and plain gains.

/// The gain of `db` decibels; minus infinity gives silence.
pub fn db_to_gain(db: f32) -> f32 {
    if db == f32::NEG_INFINITY { 0.0 } else { 10f32.powf(db / 20.0) }
}

/// The decibels of gain `g`; silence gives minus infinity.
pub fn gain_to_db(g: f32) -> f32 {
    if g <= 0.0 { f32::NEG_INFINITY } else { 20.0 * g.log10() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trips() {
        for i in 0..=720 {
            let db = -60.0 + i as f32 * 0.1;
            assert!((gain_to_db(db_to_gain(db)) - db).abs() < 1e-4, "{db}");
        }
        assert_eq!(db_to_gain(f32::NEG_INFINITY), 0.0);
        assert_eq!(gain_to_db(0.0), f32::NEG_INFINITY);
        assert!((db_to_gain(-6.0206) - 0.5).abs() < 1e-5);
    }
}
