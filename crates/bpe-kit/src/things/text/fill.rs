//! Filling a caption's tokens from the run: `{time}`, `{seconds}`, `{countdown}`, `{hits}` and
//! `{balls}`.

/// What the tokens read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Values {
    /// Seconds into the video.
    pub time: f64,
    /// The video's length in seconds.
    pub duration: f32,
    /// Hits so far.
    pub hits: u32,
    /// Balls alive now.
    pub balls: u32,
}

/// `template` with its tokens filled in: `{time}` as m:ss, `{seconds}` as whole seconds gone,
/// `{countdown}` as whole seconds left (never below 0), `{hits}` and `{balls}` as counts.
/// Anything else in braces stays as written.
pub fn fill(template: &str, v: &Values) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        match after.find('}') {
            Some(end) => {
                let token = &after[1..end];
                match token {
                    "time" => {
                        let whole = v.time.max(0.0).floor() as u64;
                        out.push_str(&format!("{}:{:02}", whole / 60, whole % 60));
                    }
                    "seconds" => out.push_str(&(v.time.max(0.0).floor() as u64).to_string()),
                    "countdown" => {
                        let left = (v.duration as f64 - v.time).max(0.0).ceil() as u64;
                        out.push_str(&left.to_string());
                    }
                    "hits" => out.push_str(&v.hits.to_string()),
                    "balls" => out.push_str(&v.balls.to_string()),
                    _ => out.push_str(&after[..=end]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_fill_from_the_run() {
        let v = Values { time: 65.5, duration: 90.0, hits: 3, balls: 2 };
        assert_eq!(fill("{time} {seconds} {countdown} {hits} {balls} {x}", &v), "1:05 65 25 3 2 {x}");
        assert_eq!(fill("plain", &v), "plain");
        assert_eq!(fill("odd {brace", &v), "odd {brace");
        assert_eq!(fill("{countdown}", &Values { time: 100.0, duration: 90.0, ..v }), "0");
        assert_eq!(fill("{time}", &Values { time: 0.0, ..v }), "0:00");
    }
}
