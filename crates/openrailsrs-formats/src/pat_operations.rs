//! Decimal native waiting-point instructions. Unknown codes are never timed waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatInstruction {
    Wait(u32),
    Until {
        hour: u32,
        minute: u32,
    },
    Detach {
        keep: usize,
        front: bool,
        wait_s: u32,
    },
    Attach,
    Permission,
    Unsupported(u32),
}
impl PatInstruction {
    pub fn decode(code: u32) -> Self {
        match code {
            0..=29999 => Self::Wait(code),
            30000..=39999 => {
                let hour = (code / 100) % 100;
                let minute = code % 100;
                if hour < 24 && minute < 60 {
                    Self::Until { hour, minute }
                } else {
                    Self::Unsupported(code)
                }
            }
            40001..=49999 | 50001..=59999 => {
                let front = code < 50000;
                let keep = ((code - if front { 40000 } else { 50000 }) / 100) as usize;
                if keep > 0 {
                    Self::Detach {
                        keep,
                        front,
                        wait_s: code % 100,
                    }
                } else {
                    Self::Unsupported(code)
                }
            }
            60001 => Self::Attach,
            60002 => Self::Permission,
            _ => Self::Unsupported(code),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pat_codes_distinguish_time_from_operations_without_fallback() {
        assert_eq!(PatInstruction::decode(90), PatInstruction::Wait(90));
        assert_eq!(
            PatInstruction::decode(32359),
            PatInstruction::Until {
                hour: 23,
                minute: 59
            }
        );
        assert_eq!(
            PatInstruction::decode(40215),
            PatInstruction::Detach {
                keep: 2,
                front: true,
                wait_s: 15
            }
        );
        assert_eq!(
            PatInstruction::decode(50320),
            PatInstruction::Detach {
                keep: 3,
                front: false,
                wait_s: 20
            }
        );
        for code in [32400, 30099, 40000, 40012, 50000, 60000, 60003, 65535] {
            assert_eq!(
                PatInstruction::decode(code),
                PatInstruction::Unsupported(code)
            );
        }
        assert_eq!(PatInstruction::decode(60001), PatInstruction::Attach);
        assert_eq!(PatInstruction::decode(60002), PatInstruction::Permission);
    }
}
