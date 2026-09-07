use anyhow::Result;
use clap::{ArgMatches, Command, arg, value_parser};
use simple_logger::SimpleLogger;

mod display;
mod volume;

fn cli() -> Command {
    Command::new(env!("CARGO_CRATE_NAME"))
        .about("Manages the Miyoo Mini hardware")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .allow_external_subcommands(true)
        .subcommand(
            Command::new("volume").arg(
                arg!([VOLUME] "Volume to set")
                    .allow_negative_numbers(true)
                    .value_parser(value_parser!(i32)),
            ),
        )
        .subcommand(
            Command::new("display")
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("blank").arg(
                        arg!([BLANK] "blank the display, or unblank it with false")
                            .value_parser(value_parser!(bool)),
                    ),
                ),
        )
}

/// Whether `display blank` should blank the display or unblank it.
///
/// An absent argument blanks. The subcommand is named `blank`, so `myctl display blank` doing the
/// opposite reads backwards; pass `false` to undo it.
fn should_blank(matches: &ArgMatches) -> bool {
    matches.get_one::<bool>("BLANK").copied().unwrap_or(true)
}

fn main() -> Result<()> {
    SimpleLogger::new().env().init().unwrap();

    let matches = cli().get_matches();

    match matches.subcommand() {
        Some(("volume", sub_matches)) => {
            if let Some(vol) = sub_matches.get_one::<i32>("VOLUME") {
                volume::set(*vol)?;
            } else {
                println!("{}", volume::get()?);
            }
        }
        Some(("display", sub_matches)) => {
            if let Some(sub_matches) = sub_matches.subcommand() {
                match sub_matches {
                    ("blank", sub_matches) => {
                        if should_blank(sub_matches) {
                            display::blank()?;
                        } else {
                            display::unblank()?;
                        }
                    }
                    _ => unreachable!(),
                }
            } else {
                unreachable!()
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walks to the `display blank` matches the way `main` does.
    ///
    /// `copied` because a slice of `&str` iterates as `&&str`, which clap will not take.
    fn blank_matches(args: &[&str]) -> ArgMatches {
        let matches = cli().try_get_matches_from(args.iter().copied()).unwrap();
        let (_, display) = matches.subcommand().unwrap();
        let (_, blank) = display.subcommand().unwrap();
        blank.clone()
    }

    #[test]
    fn blank_argument_is_read_under_the_id_it_was_declared_with() {
        // clap panics on `get_one` with an id that was never defined, so this asserts nothing
        // clever -- it just has to reach the value. The declaration once said TOGGLE while the
        // access said BLANK, which aborted the binary on every invocation.
        let matches = blank_matches(&["myctl", "display", "blank", "true"]);
        assert_eq!(matches.get_one::<bool>("BLANK"), Some(&true));

        let matches = blank_matches(&["myctl", "display", "blank", "false"]);
        assert_eq!(matches.get_one::<bool>("BLANK"), Some(&false));
    }

    #[test]
    fn blank_without_an_argument_blanks() {
        assert!(should_blank(&blank_matches(&["myctl", "display", "blank"])));
    }

    #[test]
    fn blank_follows_the_argument_when_given() {
        assert!(should_blank(&blank_matches(&[
            "myctl", "display", "blank", "true"
        ])));
        assert!(!should_blank(&blank_matches(&[
            "myctl", "display", "blank", "false"
        ])));
    }

    #[test]
    fn volume_argument_is_read_under_the_id_it_was_declared_with() {
        let matches = cli()
            .try_get_matches_from(["myctl", "volume", "-5"])
            .unwrap();
        let (_, volume) = matches.subcommand().unwrap();
        assert_eq!(volume.get_one::<i32>("VOLUME"), Some(&-5));
    }
}
