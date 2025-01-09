use anyhow::anyhow;
use cron::{Schedule, TimeUnitSpec};
use std::error::Error;
use std::str::FromStr;

pub(crate) fn cron_to_systemd(cron_expr: &str, time_zone: &str) -> Result<String, Box<dyn Error>> {
    let schedule = &format!("0 {}", cron_expr);
    let schedule = Schedule::from_str(schedule)
        .map_err(|e| anyhow!(e).context(format!("failed to parse schedule from {}", schedule)))?;

    let timer_format = format!(
        //DOW Y-M-D H:M:S TZ
        "{} *-{}-{} {}:{}:00 {}",
        linearize_time_unit(schedule.days_of_week(), ""),
        linearize_time_unit(schedule.months(), "*"),
        linearize_time_unit(schedule.days_of_month(), "*"),
        linearize_time_unit(schedule.hours(), "*"),
        linearize_time_unit(schedule.minutes(), "*"),
        time_zone,
    );

    return Ok(timer_format.trim().to_string());
}

fn linearize_time_unit(input: &(impl TimeUnitSpec + Sized), star: &str) -> String {
    if input.is_all() {
        star.to_owned()
    } else {
        let mut output = String::new();
        for part in input.iter() {
            output.push_str(&part.to_string());
            output.push(',');
        }
        output.pop();
        output
    }
}

#[cfg(test)]
mod tests {
    use crate::cron::cron_to_systemd;

    #[test]
    fn test_cron_to_systemd() {
        let conditions = &[
            ("* * * * *", "*-*-* *:*:00"),
            ("*/10 * * * *", "*-*-* *:0,10,20,30,40,50:00"),
            ("0 * * * *", "*-*-* *:0:00"),
            ("0 0 * * *", "*-*-* 0:0:00"),
            ("0 0 1 * *", "*-*-1 0:0:00"),
            ("0 0 1 1 *", "*-1-1 0:0:00"),
            ("0 0 1 1 1", "1 *-1-1 0:0:00"),
        ];

        for (input, expect) in conditions {
            match cron_to_systemd(input, "") {
                Ok(output) => {
                    assert_eq!(output, *expect, "input: {}", input);
                }
                Err(e) => {
                    panic!("{}: {}", *expect, e);
                }
            }
        }
    }
}
