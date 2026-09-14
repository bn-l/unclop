// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Someone

//! Crate level doc comment that explains the module.

use std::collections::HashMap;

// ---------------------------------------------------------------------------

/// Utilizes the robust helper to seamlessly parse the input.
/// Second line of the doc.
pub fn process_validated_user_data_result(validated_input: &str, ctx: &Ctx) -> Result<Output, Error> {
    // Leverage the helper to compute the final result
    // spread over two lines
    let processed_result = helper(validated_input); // trailing note
    let (first, second) = split(processed_result);
    let Point { x, y: renamed } = origin();
    let Some(inner) = maybe else { return Err(Error::Missing) };
    for (index, item) in items.iter().enumerate() {
        /* block comment inside loop */
    }
    let closure = |a, (b, c): (i32, i32)| a + b + c;
    log::info!("Successfully processed the user data with great care");
    println!("{}", "short");
    Ok(Output { value: format!("Value is {first}") })
}

pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub enum Error {
    /// The value was missing.
    Missing,
    Invalid { reason: String },
}

pub trait Parser {
    fn parse_thing(&self, input: &str) -> i32;
}

impl Parser for Point {
    fn parse_thing(&self, input: &str) -> i32 {
        0
    }
}

impl Point {
    pub fn new(x: i32) -> Self {
        Self { x, y: 0 }
    }
}

const MAX_RETRY_COUNT: u32 = 3;
static GREETING: &str = "Hello there, welcome aboard";

mod inner_module {
    pub fn hidden() {}
}

macro_rules! make_thing {
    () => {};
}

#[allow(dead_code)]
#[doc = "attribute string that must not be listed"]
fn attributed() {}
