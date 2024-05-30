#![doc = include_str!("../README.md")]

use std::{
    collections::HashSet,
    process::{Command, ExitCode},
};

use clap::Parser;
use itertools::Itertools;
use sysinfo::{Pid, System};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
enum ParseByteError {
    #[error("cannot parse empty string")]
    Empty,
    #[error("unexpected alphabetic character before amount")]
    AlphaBeforeAmount,
    #[error(transparent)]
    ParseIntError(#[from] std::num::ParseIntError),
    #[error("unknown prefix '{}'", .0)]
    UnknownPrefix(String),
    #[error("specified unit '{}' too big for current architecture", .0)]
    UnitOverflow(String),
    #[error("amount '{}{}' too big for current architecture", .0, .1)]
    AmountOverflow(usize, String),
    #[error("unexpected string '{}' after unit", .0)]
    UnexpectedEnd(String),
}

// using usize here because on 32-bit platforms it doesn't make sense to limit to >4GB of RAM
fn parse_byte_amount(s: &str) -> Result<usize, ParseByteError> {
    let s = s.trim();

    let groups = s.chars().chunk_by(|c| c.is_alphabetic());
    let mut groups = groups.into_iter().map(|(b, g)| (b, g.collect::<String>()));

    let (is_alpha, group) = groups.next().ok_or(ParseByteError::Empty)?;

    let amount = if is_alpha {
        Err(ParseByteError::AlphaBeforeAmount)
    } else {
        group.parse::<usize>().map_err(Into::into)
    }?;

    if let Some((is_alpha, group)) = groups.next() {
        assert!(is_alpha);

        let (base, pow): (usize, usize) = match group.as_str() {
            "B" => Ok((1, 1)),
            "K" | "KB" => Ok((1000, 1)),
            "Ki" | "KiB" => Ok((1024, 1)),
            "M" | "MB" => Ok((1000, 2)),
            "Mi" | "MiB" => Ok((1024, 2)),
            "G" | "GB" => Ok((1000, 3)),
            "Gi" | "GiB" => Ok((1024, 3)),
            "T" | "TB" => Ok((1000, 4)),
            "Ti" | "TiB" => Ok((1024, 4)),
            "P" | "PB" => Ok((1000, 5)),
            "Pi" | "PiB" => Ok((1024, 5)),
            "E" | "EB" => Ok((1000, 6)),
            "Ei" | "EiB" => Ok((1024, 6)),
            "Z" | "ZB" => Ok((1000, 7)),
            "Zi" | "ZiB" => Ok((1024, 7)),
            "Y" | "YB" => Ok((1000, 8)),
            "Yi" | "YiB" => Ok((1024, 8)),
            "R" | "RB" => Ok((1000, 9)),
            "Ri" | "RiB" => Ok((1024, 9)),
            "Q" | "QB" => Ok((1000, 10)),
            "Qi" | "QiB" => Ok((1024, 10)), // if anyone uses this program at the point in time where 1000 quettabytes isn't enough let me know (if i'm not dead yet)
            _ => Err(ParseByteError::UnknownPrefix(group.clone())),
        }?;

        let multiplier = base
            .checked_pow(pow as u32)
            .ok_or(ParseByteError::UnitOverflow(group.clone()))?;

        let amount = amount
            .checked_mul(multiplier)
            .ok_or(ParseByteError::AmountOverflow(amount, group))?;

        let rest = groups.map(|(_, s)| s).collect::<Vec<String>>().join("");

        if rest.is_empty() {
            Ok(amount)
        } else {
            Err(ParseByteError::UnexpectedEnd(rest))
        }
    } else {
        Ok(amount)
    }
}

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// The maximum amount of memory before being killed. Either in raw byte amounts (e.g. "300"), or with a unit (e.g. "300B" or "300KB" or "300KiB").
    #[clap(value_parser = parse_byte_amount)]
    amount: usize,
    /// Monitor virtual memory instead of resident set memory.
    #[arg(name = "virtual", long)]
    virtual_mem: bool,
    /// Monitor the sum of all memory consumption from all children of the process.
    #[arg(short, long)]
    children: bool,
    /// The command to watch
    command: String,
    /// Arguments to the watched command
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let mut command = Command::new(args.command).args(args.args).spawn().unwrap();
    let pid = Pid::from_u32(command.id());

    let mut sys = System::new_all();
    sys.refresh_all();

    // list of process + childrens
    let mut hierarchy_pids = HashSet::new();

    while sys.process(pid).is_some() {
        // i am the ancestor of my childrens
        hierarchy_pids.insert(pid);

        if args.children {
            loop {
                let old_len = hierarchy_pids.len();

                // get the children of all the processes in the hashset
                let v = sys
                    .processes()
                    .iter()
                    .filter_map(|(id, process)| {
                        if let Some(p) = process.parent() {
                            if hierarchy_pids.contains(&p) {
                                Some(*id)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                // add them to the hashset
                hierarchy_pids.extend(v);

                // if no change in this iteration, then all children found
                if old_len == hierarchy_pids.len() {
                    break;
                }
            }
        }

        let mem = sys
            .processes()
            .iter()
            .filter_map(|(pid, process)| {
                if hierarchy_pids.contains(pid) {
                    if args.virtual_mem {
                        Some(process.virtual_memory())
                    } else {
                        Some(process.memory())
                    }
                } else {
                    None
                }
            })
            .sum::<u64>(); // can't overflow ever since we can't have more than 2^64 bytes of memory anyways

        if mem as usize > args.amount {
            command.kill().unwrap();
            println!(
                "memlimit: memory usage = {mem} bytes, higher than limit of {}, killed.",
                args.amount
            );
            break;
        }

        sys.refresh_processes();
        // clear the list since old processes aren't relevant anymore
        hierarchy_pids.clear();
    }

    // return the same exit code as child
    command
        .wait()
        .unwrap()
        .code()
        .map(|e| ExitCode::from(e as u8))
        .unwrap_or(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert_eq!(parse_byte_amount(""), Err(ParseByteError::Empty));
    }

    #[test]
    fn test_whitespace() {
        assert_eq!(
            parse_byte_amount("              "),
            Err(ParseByteError::Empty)
        );
    }

    #[test]
    fn test_number_only() {
        assert_eq!(parse_byte_amount("3"), Ok(3));
    }

    #[test]
    fn test_number_only_too_big() {
        assert!(matches!(
            parse_byte_amount("3000000000000000000000000000000000000000000000000000000"),
            Err(ParseByteError::ParseIntError(_))
        ));
    }

    #[test]
    fn test_stuff_after_unit() {
        assert_eq!(
            parse_byte_amount("3GiB1234244hello"),
            Err(ParseByteError::UnexpectedEnd(String::from("1234244hello")))
        );
    }

    #[test]
    fn test_number_only_negative() {
        assert!(matches!(
            parse_byte_amount("-1"),
            Err(ParseByteError::ParseIntError(_))
        ));
    }

    #[test]
    fn test_number_byte() {
        assert_eq!(parse_byte_amount("3B"), Ok(3));
    }

    #[test]
    fn test_number_kilobyte() {
        assert_eq!(parse_byte_amount("5K"), Ok(5000));
        assert_eq!(parse_byte_amount("5KB"), Ok(5000));
    }

    #[test]
    fn test_number_kibibyte() {
        assert_eq!(parse_byte_amount("5Ki"), Ok(5120));
        assert_eq!(parse_byte_amount("5KiB"), Ok(5120));
    }

    #[test]
    fn test_unit_too_big() {
        assert_eq!(
            parse_byte_amount("1QB"),
            Err(ParseByteError::UnitOverflow(String::from("QB")))
        );
    }

    #[test]
    fn test_resulting_value_too_big() {
        assert_eq!(
            parse_byte_amount("10000000000GB"),
            Err(ParseByteError::AmountOverflow(
                10000000000,
                String::from("GB")
            ))
        );
    }
}
