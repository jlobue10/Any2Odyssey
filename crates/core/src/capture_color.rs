//! Separate an unknown display query from a confirmed color-mode change.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureColor {
    pub advanced_color: bool,
    pub white_scale: f32,
}

impl CaptureColor {
    /// Startup fallback only: retain FP16 when the display query is unavailable.
    pub const CONSERVATIVE: Self = Self {
        advanced_color: true,
        white_scale: 1.0,
    };
}

/// Queries run once a second. A brief failure leaves the working pipeline
/// alone; a persistent failure stops it rather than trusting stale color data.
pub struct ColorMonitor {
    color: CaptureColor,
    failures: u8,
}

impl ColorMonitor {
    pub const MAX_QUERY_FAILURES: u8 = 5;

    pub fn new(initial: Option<CaptureColor>) -> Self {
        Self {
            color: initial.unwrap_or(CaptureColor::CONSERVATIVE),
            failures: 0,
        }
    }

    pub fn color(&self) -> CaptureColor {
        self.color
    }

    pub fn observe(&mut self, result: Option<CaptureColor>) -> Result<(), &'static str> {
        if let Some(color) = result {
            self.failures = 0;
            if color != self.color {
                return Err("Display color mode or SDR white level changed. Restart conversion.");
            }
        } else {
            self.failures = self.failures.saturating_add(1);
            if self.failures >= Self::MAX_QUERY_FAILURES {
                return Err("Display color information remained unavailable. Restart conversion.");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDR: CaptureColor = CaptureColor {
        advanced_color: false,
        white_scale: 1.0,
    };

    #[test]
    fn temporary_query_failure_does_not_become_a_color_change() {
        let mut monitor = ColorMonitor::new(Some(SDR));
        for _ in 0..3 {
            for _ in 0..ColorMonitor::MAX_QUERY_FAILURES - 1 {
                assert_eq!(monitor.observe(None), Ok(()));
                assert_eq!(monitor.color(), SDR);
            }
            assert_eq!(monitor.observe(Some(SDR)), Ok(()));
        }
    }

    #[test]
    fn persistent_unknown_state_stops_instead_of_using_stale_data_forever() {
        let mut monitor = ColorMonitor::new(Some(SDR));
        for _ in 0..ColorMonitor::MAX_QUERY_FAILURES - 1 {
            assert!(monitor.observe(None).is_ok());
        }
        assert!(monitor.observe(None).unwrap_err().contains("unavailable"));
    }

    #[test]
    fn confirmed_mode_and_white_level_changes_still_require_recreation() {
        let hdr = CaptureColor {
            advanced_color: true,
            white_scale: 2.5,
        };
        assert!(ColorMonitor::new(Some(SDR)).observe(Some(hdr)).is_err());
        assert!(ColorMonitor::new(Some(hdr))
            .observe(Some(CaptureColor {
                white_scale: 3.0,
                ..hdr
            }))
            .is_err());
    }

    #[test]
    fn unknown_startup_remains_conservative_until_a_real_result() {
        let mut monitor = ColorMonitor::new(None);
        assert_eq!(monitor.color(), CaptureColor::CONSERVATIVE);
        assert!(monitor.observe(None).is_ok());
        assert!(monitor.observe(Some(SDR)).is_err());
    }
}
