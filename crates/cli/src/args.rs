//! Hand-written argument parsing, with French messages: after the command, options and
//! positional arguments in any order, `--name value` or `--name=value`.

use crate::Failure;

/// A command's arguments once parsed.
pub(crate) struct Args {
    pub positionals: Vec<String>,
    flags: Vec<&'static str>,
    values: Vec<(&'static str, String)>,
}

/// Parses `args` (what follows the command) knowing the command's flags and the options that
/// take a value. `--help` and `-h` are always known.
pub(crate) fn parse(args: &[String], flags: &[&'static str], valued: &[&'static str]) -> Result<Args, Failure> {
    let mut parsed = Args { positionals: vec![], flags: vec![], values: vec![] };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !a.starts_with('-') || a.len() == 1 {
            parsed.positionals.push(a.clone());
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (a.as_str(), None),
        };
        if let Some(&f) = flags.iter().chain(&["--help", "-h"]).find(|f| **f == name) {
            if inline.is_some() {
                return Err(Failure::Usage(format!("{f} ne prend pas de valeur")));
            }
            parsed.flags.push(f);
        } else if let Some(&v) = valued.iter().find(|v| **v == name) {
            let value = inline.or_else(|| it.next().cloned()).ok_or_else(|| Failure::Usage(format!("{v} attend une valeur")))?;
            parsed.values.push((v, value));
        } else {
            return Err(Failure::Usage(format!("option inconnue : {name}")));
        }
    }
    Ok(parsed)
}

impl Args {
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }

    /// The option's value (the last one if given several times).
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v.as_str())
    }

    pub fn help(&self) -> bool {
        self.flag("--help") || self.flag("-h")
    }

    /// Fails if there are more than `n` positional arguments.
    pub fn at_most(&self, n: usize) -> Result<(), Failure> {
        match self.positionals.get(n) {
            Some(extra) => Err(Failure::Usage(format!("argument en trop : « {extra} »"))),
            None => Ok(()),
        }
    }
}
