pub fn build_elapsed_timeline(duration: u64, step: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut t = 0u64;

    while t <= duration {
        out.push(t);
        t = t.saturating_add(step);
        if step == 0 {
            break;
        }
    }

    if out.last().copied().unwrap_or(0) != duration {
        out.push(duration);
    }

    out
}
