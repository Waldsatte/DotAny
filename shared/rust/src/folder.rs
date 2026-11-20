//! The files beside this one, in the order a person expects: 2 before 10, ignoring case.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

fn chunks(name: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    for c in name.chars() {
        let digit = c.is_ascii_digit();
        match out.last_mut() {
            Some((d, s)) if *d == digit => s.push(c),
            _ => out.push((digit, c.to_string())),
        }
    }
    out
}

pub fn natural(a: &str, b: &str) -> Ordering {
    let (x, y) = (chunks(a), chunks(b));
    for (p, q) in x.iter().zip(y.iter()) {
        let o = match (p.0, q.0) {
            (true, true) => {
                let (m, n) = (p.1.trim_start_matches('0'), q.1.trim_start_matches('0'));
                m.len().cmp(&n.len()).then_with(|| m.cmp(n)).then_with(|| q.1.len().cmp(&p.1.len()))
            }
            (false, false) => p.1.to_lowercase().cmp(&q.1.to_lowercase()),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    x.len().cmp(&y.len()).then_with(|| a.cmp(b))
}

/// Every file in this file's folder that `allowed` accepts by name, in order.
pub fn files_beside(path: &Path, allowed: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else { return Vec::new() };
    let Ok(list) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = list
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| allowed(n) && !n.starts_with(crate::atomic::TEMP_PREFIX))
        .collect();
    names.sort_by(|a, b| natural(a, b));
    names.into_iter().map(|n| dir.join(n)).collect()
}

/// The next file along, wrapping at the ends. A file that has just disappeared still has a place: where its name would sort.
pub fn neighbour(path: &Path, step: i32, allowed: impl Fn(&str) -> bool) -> Option<PathBuf> {
    let all = files_beside(path, allowed);
    if all.is_empty() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let n = all.len() as i64;
    let names: Vec<String> = all.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    let here = names.iter().position(|p| crate::platform::same_name(p.as_ref(), name.as_ref()));
    let to = match here {
        Some(i) => (i as i64 + step as i64).rem_euclid(n),
        None => {
            let after = names.iter().position(|p| natural(p, &name) == Ordering::Greater).unwrap_or(all.len()) as i64;
            if step > 0 { after.rem_euclid(n) } else { (after - 1).rem_euclid(n) }
        }
    };
    let found = all[to as usize].clone();
    if here.is_some() && all.len() == 1 {
        return None;
    }
    Some(found)
}

pub fn count_beside(path: &Path, allowed: impl Fn(&str) -> bool) -> usize {
    files_beside(path, allowed).len()
}
