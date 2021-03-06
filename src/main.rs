#![feature(plugin, slice_patterns, convert, str_utf16)]
#![plugin(regex_macros)]

extern crate rand;
extern crate regex;
extern crate rustc_serialize;
extern crate bincode;

#[cfg(windows)]
extern crate winapi;
#[cfg(windows)]
extern crate ole32;

use std::fs::{self, File};
use std::io::{self, Write, BufReader, BufRead};
use std::path::{Path, PathBuf};
use std::convert::AsRef;
use std::fmt::Debug;
use std::iter::Extend;
use training::{Trainer, StrTrainer, MultilineTrainer};

pub mod chain;
pub mod training;

#[cfg(windows)]
pub mod tts;
#[cfg(not(windows))]
#[path(file = "nop_tts.rs")]
pub mod tts;

fn handle_input(input: &String, chain: &mut chain::Chain) -> bool {
    let split: Vec<_> = input.splitn(2, ' ').collect();
    match split.get(0).cloned() {
        Some("quit") => {
            return false;
        }
        Some("stats") => {
            chain.print_stats();
        }
        Some("generate") => {
            let mut words = 50;
            if let Some(args) = split.get(1) {
                if let Ok(num) = args.parse() {
                    words = num;
                }
            }
            generate(chain, 1, words);
        }
        Some("speechify") => {
            let sentence_end = regex!(r"[\.\?!]");

            let mut speechify = tts::Speechifier::new();
            speechify.start();

            let mut buffer = String::new();
            for word in chain.iter() {
                buffer.extend(word.chars());
                buffer.push(' ');

                if sentence_end.is_match(word) {
                    speechify.queue(buffer.clone());
                    buffer.clear();
                }

                print!("{} ", word);
                io::stdout().flush().unwrap();
            }

            speechify.stop();
        }
        Some("save") => {
            save_chain(chain);
            println!("saved!");
        }
        Some("train") => {
            if let Some(data) = split.get(1) {
                let mut trainer = StrTrainer::new(&data);
                trainer.train(chain);
            } else {
                println!("Why not give me a sentence to train from?");
            }
        }
        Some("train-file") => {
            if let Some(filename) = split.get(1) {
                train_file(chain, filename);
            } else {
                println!("You should give me a filename!");
            }
        }
        Some("train-lines") => {
            let stdin = io::stdin();
            train_reader(chain, &mut stdin.lock(), |line| {
                line.find("<<<") == Some(0)
            });
        }
        Some("train-all") => {
            if let Some(args) = split.get(1) {
                let args: Vec<_> = args.splitn(2, ' ').collect();
                if let [folder, pattern] = args.as_slice() {
                    let pattern = match regex::Regex::new(pattern) {
                        Ok(pattern) => pattern,
                        Err(e) => {
                            println!("Your regex is bad and you should feel bad: {:?}", e);
                            return true;
                        }
                    };

                    all_files(folder, &pattern, |path| {
                        println!("Training from {:?}", path.as_path());
                        train_file(chain, &path);
                    });

                    return true;
                }
            }

            println!("Usage: train-all [folder] [pattern]");
        }
        Some("list-choices") => {
            if let Some(args) = split.get(1) {
                let args: Vec<_> = args.split_whitespace().collect();
                if let [arg0, arg1, arg2] = args.as_slice() {
                    let lookup = (arg0, arg1, arg2);
                    if let Some(choices) = chain.lookup_choices(lookup) {
                        for choice in choices.choices.iter() {
                            let name = chain.get_name(choice.item).unwrap();
                            println!("{:?} {} | weight: {}", lookup, name, choice.weight);
                        }
                    } else {
                        println!("I've never seen that combination! You should tell me more :3");
                    }
                } else {
                    println!("Please specify 3 words");
                }
            } else {
                println!("Usage: list-choices word0 word1 word2");
                println!("Use \"\" to represent the beginning");
            }
        }
        cmd => {
            if let Some(cmd) = cmd {
                println!("I don't know what `{}` means", cmd);
            } else {
                println!("Please type a command");
            }
        }
    };
    true
}

fn main() {
    let mut chain = load_chain();

    chain.clear_empty();

    print!("> ");
    io::stdout().flush().unwrap();

    let newline = regex!("[\r\n]+");

    let stdin = io::stdin();
    let mut input = String::new();
    while let Ok(_) = stdin.read_line(&mut input) {
        input = newline.replace(&input, "");

        if !handle_input(&input, &mut chain) {
            return;
        }

        input.clear();
        print!("> ");
        io::stdout().flush().unwrap();
    }
}

fn filename() -> &'static str {
    "chains.markov"
}

fn load_chain() -> chain::Chain {
    let mut chain = chain::Chain::new();
    if let Ok(mut file) = File::open(filename()) {
        chain.load(&mut file)
    }
    chain
}

fn save_chain(chain: &mut chain::Chain) {
    chain.save(&mut File::create(filename()).unwrap());
}

fn generate(chain: &chain::Chain, num_sentences: usize, max_words: usize) {
    for _ in 0..num_sentences {
        print!("{}", chain.generate_sequence(max_words));
    }
    println!("");
}

fn train_reader<R: BufRead, F>(chain: &mut chain::Chain, reader: &mut R, break_pred: F)
        where F: Fn(&String) -> bool {
    let mut line = String::new();
    let mut trainer = MultilineTrainer::new(chain);

    while let Ok(n) = reader.read_line(&mut line) {
        if n == 0 { break; }
        if break_pred(&line) { break; }

        trainer.next(&line).train(chain);
        line.clear();
    }
}

fn train_file<P: AsRef<Path> + Debug>(chain: &mut chain::Chain, path: &P) {
    if let Ok(file) = File::open(path) {
        let mut reader = BufReader::new(file);
        train_reader(chain, &mut reader, |_| false);
    } else {
        println!("I couldn't open {:?}", path);
    }
}

fn all_files<F>(folder: &str, pattern: &regex::Regex, mut cb: F) where F: FnMut(PathBuf) {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) => {
            println!("I couldn't find anything in {}: {:?}", folder, e);
            return;
        }
    };

    for entry in entries {
        if let Ok(entry) = entry {
            if let Ok(ftype) = entry.file_type() {
                if !ftype.is_file() {
                    continue;
                }
            } else {
                continue;
            }

            let file_name = if let Ok(name) = entry.file_name().into_string() {
                name
            } else {
                continue;
            };

            if pattern.is_match(&file_name) {
                cb(entry.path());
            }
        }
    }
}
