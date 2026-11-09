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
    /// False while `color` is the startup fallback rather than a query result.
    confirmed: bool,
}

impl ColorMonitor {
    pub const MAX_QUERY_FAILURES: u8 = 5;

    pub fn new(initial: Option<CaptureColor>) -> Self {
        Self {
            color: initial.unwrap_or(CaptureColor::CONSERVATIVE),
            failures: 0,
            confirmed: initial.is_some(),
        }
    }

    pub fn color(&self) -> CaptureColor {
        self.color
    }

    pub fn observe(&mut self, result: Option<CaptureColor>) -> Result<(), &'static str> {
        if let Some(color) = result {
            self.failures = 0;
            let confirmed = std::mem::replace(&mut self.confirmed, true);
            if color != self.color {
                // The pipeline compiles the white level into its shader, so a
                // first real answer that differs from the startup fallback
                // still needs a rebuild; it is not a change the user made.
                return Err(if confirmed {
                    "Display color mode or SDR white level changed. Restart conversion."
                } else {
                    "Display color information became available after startup; restarting conversion to use it."
                });
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
        let message = monitor.observe(Some(SDR)).unwrap_err();
        assert!(message.contains("became available"), "{message}");
        assert!(!message.contains("changed"), "{message}");
    }

    #[test]
    fn unknown_startup_confirmed_by_the_fallback_itself_is_quiet() {
        let mut monitor = ColorMonitor::new(None);
        assert_eq!(monitor.observe(Some(CaptureColor::CONSERVATIVE)), Ok(()));
        let hdr = CaptureColor {
            advanced_color: true,
            white_scale: 2.0,
        };
        assert!(monitor.observe(Some(hdr)).unwrap_err().contains("changed"));
    }
}
