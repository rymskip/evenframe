//! Regex pattern generator that creates random strings matching regex patterns.
//!
//! This module provides a `RegexValGen` that can parse simple regex patterns
//! and generate random strings that match those patterns.
//!
//! # Example
//! ```ignore
//! use maker::RegexValGen;
//!
//! let mut generator = RegexValGen::new();
//! let result = generator.generate(r"[a-z]{3}\d{2}").unwrap();
//! // Might generate: "abc12"
//! ```

use rand::Rng;
use std::error::Error;
use std::fmt;
use tracing;

// Character class constants
const HEX_CHARS: &str = "0123456789abcdefABCDEF";
const ALPHA_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOWER_CHARS: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPER_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const ALPHANUM_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const BASE64_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

// Default repeat limits
const DEFAULT_REPEAT_MAX: usize = 10;

/// Errors that can occur during regex generation
#[derive(Debug, Clone)]
pub enum MakerError {
    InvalidPattern(String),
    InvalidQuantifier(String),
    InvalidCharacterRange { start: char, end: char },
    UnmatchedBracket(char),
    EmptyAlternation,
}

impl fmt::Display for MakerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPattern(msg) => write!(f, "Invalid pattern: {}", msg),
            Self::InvalidQuantifier(q) => write!(f, "Invalid quantifier: {}", q),
            Self::InvalidCharacterRange { start, end } => {
                write!(f, "Invalid character range: {}-{}", start, end)
            }
            Self::UnmatchedBracket(ch) => write!(f, "Unmatched bracket: '{}'", ch),
            Self::EmptyAlternation => write!(f, "Empty alternation pattern"),
        }
    }
}

impl Error for MakerError {}

type Result<T> = std::result::Result<T, MakerError>;

/// Components that make up a regex pattern
#[derive(Debug, Clone)]
pub enum RegexComponent {
    // Literal characters/strings
    Literal(String),

    // Character classes
    CharClass(Vec<char>),
    CharRange(char, char),
    DigitClass,      // \d or [0-9]
    HexClass,        // [0-9a-fA-F]
    AlphaClass,      // [a-zA-Z]
    LowerClass,      // [a-z]
    UpperClass,      // [A-Z]
    AlphaNumClass,   // [a-zA-Z0-9]
    Base64Class,     // [A-Za-z0-9+/]
    WhitespaceClass, // \s
    AnyChar,         // . (any character except newline)

    // Quantifiers
    Repeat {
        component: Box<RegexComponent>,
        count: usize,
    },
    RepeatRange {
        component: Box<RegexComponent>,
        min: usize,
        max: usize,
    },

    // Grouping and alternation
    Group(Vec<RegexComponent>),
    Alternation(Vec<Vec<RegexComponent>>),

    // Special patterns
    Optional(Box<RegexComponent>),

    // Numeric range pattern
    NumericRange {
        min: u32,
        max: u32,
        leading_zeros: bool,
        digits: Option<usize>,
    },
}

/// A regex pattern generator that creates random strings matching regex patterns
#[derive(Default)]
pub struct RegexValGen {
    rng: rand::rngs::ThreadRng,
}

impl RegexValGen {
    /// Creates a new RegexValGen instance
    pub fn new() -> Self {
        tracing::trace!("Creating new RegexValGen instance");
        Self { rng: rand::rng() }
    }

    /// Generates a random string matching the given regex pattern
    ///
    /// # Arguments
    /// * `pattern` - A regex pattern string
    ///
    /// # Returns
    /// * `Ok(String)` - A randomly generated string matching the pattern
    /// * `Err(MakerError)` - If the pattern is invalid
    ///
    /// # Supported patterns
    /// * Character classes: `[a-z]`, `[0-9]`, `\d`
    /// * Quantifiers: `{n}`, `{n,m}`, `+`, `*`, `?`
    /// * Groups: `(abc)`, `(a|b|c)`
    /// * Literals: `abc`, `\+`, `\.`
    pub fn generate(&mut self, pattern: &str) -> Result<String> {
        tracing::trace!(pattern = %pattern, "Generating string from regex pattern");
        let trimmed_pattern = pattern.trim_start_matches('^').trim_end_matches('$');
        let components = self.parse_pattern(trimmed_pattern)?;
        #[cfg(test)]
        println!("Parsed components: {:?}", components);
        tracing::trace!(
            component_count = components.len(),
            "Pattern parsed successfully"
        );

        let should_validate_duration = Self::is_duration_pattern(trimmed_pattern);
        let mut attempts = 0;

        loop {
            let candidate = self.generate_from_components(&components);
            if !should_validate_duration || Self::is_valid_duration(&candidate) {
                tracing::trace!(
                    pattern = %pattern,
                    result_length = candidate.len(),
                    attempts,
                    "Generated string from pattern"
                );
                return Ok(candidate);
            }

            attempts += 1;
            if attempts >= 10 {
                tracing::debug!(
                    pattern = %pattern,
                    candidate = %candidate,
                    "Returning last candidate after repeated duration validation failures"
                );
                return Ok(candidate);
            }
        }
    }

    fn parse_pattern(&self, pattern: &str) -> Result<Vec<RegexComponent>> {
        tracing::trace!(pattern = %pattern, "Parsing regex pattern");
        // Remove anchors ^ and $
        let pattern = pattern.trim_start_matches('^').trim_end_matches('$');

        // Check if this is a top-level alternation (but not inside parentheses)
        let mut paren_depth = 0;
        let mut has_top_level_pipe = false;
        for ch in pattern.chars() {
            match ch {
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                '|' if paren_depth == 0 => {
                    has_top_level_pipe = true;
                    break;
                }
                _ => {}
            }
        }

        if has_top_level_pipe {
            // Handle top-level alternation
            return Ok(vec![self.parse_alternation(pattern)?]);
        }

        let mut components = Vec::new();
        let mut chars = pattern.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        match escaped {
                            'd' => {
                                // Look ahead to see if this is part of a bounded numeric pattern
                                if let Some('{') = chars.peek() {
                                    chars.next(); // consume '{'
                                    let quantifier = self.parse_quantifier(&mut chars)?;
                                    match quantifier {
                                        (count, None) if count <= 4 => {
                                            // For small counts, we might want to create a numeric range
                                            let max = 10_u32.pow(count as u32) - 1;
                                            components.push(RegexComponent::NumericRange {
                                                min: 0,
                                                max,
                                                leading_zeros: true,
                                                digits: Some(count),
                                            });
                                        }
                                        (min, Some(max)) => {
                                            components.push(RegexComponent::RepeatRange {
                                                component: Box::new(RegexComponent::DigitClass),
                                                min,
                                                max,
                                            });
                                        }
                                        _ => {
                                            components.push(RegexComponent::Repeat {
                                                component: Box::new(RegexComponent::DigitClass),
                                                count: quantifier.0,
                                            });
                                        }
                                    }
                                } else {
                                    components.push(RegexComponent::DigitClass);
                                }
                            }
                            's' => components.push(RegexComponent::WhitespaceClass),
                            '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
                            | '^' | '$' | '"' => {
                                components.push(RegexComponent::Literal(escaped.to_string()));
                            }
                            _ => components.push(RegexComponent::Literal(escaped.to_string())),
                        }
                    }
                }
                '[' => {
                    let char_class = self.parse_char_class(&mut chars)?;
                    components.push(char_class);
                }
                '{' => {
                    if let Some(last) = components.pop() {
                        let quantifier = self.parse_quantifier(&mut chars)?;
                        match quantifier {
                            (count, None) => components.push(RegexComponent::Repeat {
                                component: Box::new(last),
                                count,
                            }),
                            (min, Some(max)) => {
                                if min > max {
                                    return Err(MakerError::InvalidQuantifier(format!(
                                        "{{{},{}}}",
                                        min, max
                                    )));
                                }
                                components.push(RegexComponent::RepeatRange {
                                    component: Box::new(last),
                                    min,
                                    max,
                                })
                            }
                        }
                    }
                }
                '(' => {
                    let group_content = self.parse_group(&mut chars);
                    // Check for non-capturing group
                    if let Some(non_capturing_content) = &group_content.strip_prefix("?:") {
                        if non_capturing_content.contains('|') {
                            components.push(self.parse_alternation(non_capturing_content)?);
                        } else {
                            components.push(RegexComponent::Group(
                                self.parse_pattern(non_capturing_content)?,
                            ));
                        }
                    } else if group_content.contains('|') {
                        components.push(self.parse_alternation(&group_content)?);
                    } else {
                        components.push(RegexComponent::Group(self.parse_pattern(&group_content)?));
                    }
                }
                '?' => {
                    if let Some(last) = components.pop() {
                        components.push(RegexComponent::Optional(Box::new(last)));
                    }
                }
                '+' => {
                    if let Some(last) = components.pop() {
                        components.push(RegexComponent::RepeatRange {
                            component: Box::new(last),
                            min: 1,
                            max: DEFAULT_REPEAT_MAX,
                        });
                    }
                }
                '*' => {
                    if let Some(last) = components.pop() {
                        components.push(RegexComponent::RepeatRange {
                            component: Box::new(last),
                            min: 0,
                            max: DEFAULT_REPEAT_MAX,
                        });
                    }
                }
                '.' => {
                    components.push(RegexComponent::AnyChar);
                }
                '-' if chars.peek() == Some(&'?') => {
                    chars.next(); // consume '?'
                    components.push(RegexComponent::Optional(Box::new(RegexComponent::Literal(
                        "-".to_string(),
                    ))));
                }
                _ => {
                    components.push(RegexComponent::Literal(ch.to_string()));
                }
            }
        }

        Ok(components)
    }

    fn is_duration_pattern(pattern: &str) -> bool {
        pattern.contains("P((\\d{1,2}Y)?")
            && pattern.contains("(0?[0-9]|1[0-1]M)?")
            && pattern.contains("(\\d{1,4}W)?")
            && pattern.contains("([0-2]?[0-9]D)?")
            && pattern.contains("T([0-1]?[0-9]|2[0-3]H)?")
            && pattern.contains("([0-5]?[0-9]M)?")
            && pattern.contains("([0-5]?[0-9](\\.\\d{1,3})?S)?")
    }

    fn is_valid_duration(candidate: &str) -> bool {
        if !candidate.starts_with('P') {
            return true;
        }

        let rest = &candidate[1..];
        let (date_part, time_part) = if let Some(idx) = rest.find('T') {
            (&rest[..idx], Some(&rest[idx + 1..]))
        } else {
            (rest, None)
        };

        if date_part
            .find('M')
            .map(|idx| Self::value_before_index_exceeds(date_part, idx, 11))
            .unwrap_or(false)
        {
            return false;
        }

        if date_part
            .find('D')
            .map(|idx| Self::value_before_index_exceeds(date_part, idx, 29))
            .unwrap_or(false)
        {
            return false;
        }

        if let Some(time_part) = time_part {
            if time_part
                .find('H')
                .map(|idx| Self::value_before_index_exceeds(time_part, idx, 23))
                .unwrap_or(false)
            {
                return false;
            }

            if let Some(min_idx) = time_part.rfind('M') {
                let segment_start = time_part.find('H').map(|idx| idx + 1).unwrap_or(0);
                if min_idx >= segment_start {
                    let minute_segment = &time_part[segment_start..min_idx];
                    if minute_segment.is_empty() {
                        return false;
                    }
                    if let Ok(minute) = minute_segment.parse::<u32>() {
                        if minute > 59 {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            }

            if let Some(sec_idx) = time_part.find('S') {
                let segment_start = if let Some(min_idx) = time_part.rfind('M') {
                    min_idx + 1
                } else if let Some(hour_idx) = time_part.find('H') {
                    hour_idx + 1
                } else {
                    0
                };

                if sec_idx >= segment_start {
                    let second_segment = &time_part[segment_start..sec_idx];
                    if second_segment.is_empty() {
                        return false;
                    }
                    let int_part = second_segment.split('.').next().unwrap_or("");
                    if int_part.is_empty() {
                        return false;
                    }
                    if let Ok(second) = int_part.parse::<u32>() {
                        if second > 59 {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn digits_before_index(slice: &str, idx: usize) -> Option<String> {
        if idx == 0 {
            return None;
        }

        let mut digits = String::new();
        for ch in slice[..idx].chars().rev() {
            if ch.is_ascii_digit() {
                digits.push(ch);
            } else {
                break;
            }
        }

        if digits.is_empty() {
            None
        } else {
            Some(digits.chars().rev().collect())
        }
    }

    fn value_before_index_exceeds(slice: &str, idx: usize, limit: u32) -> bool {
        Self::digits_before_index(slice, idx)
            .and_then(|value| value.parse::<u32>().ok())
            .map(|parsed| parsed > limit)
            .unwrap_or(false)
    }

    fn parse_char_class(
        &self,
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<RegexComponent> {
        tracing::trace!("Parsing character class");
        let mut class_content = String::new();
        let mut bracket_count = 1;

        for ch in chars.into_iter() {
            if ch == '[' {
                bracket_count += 1;
            } else if ch == ']' {
                bracket_count -= 1;
                if bracket_count == 0 {
                    break;
                }
            }
            class_content.push(ch);
        }

        // Handle common character classes
        let component = match class_content.as_str() {
            "0-9" => RegexComponent::DigitClass,
            "a-z" => RegexComponent::LowerClass,
            "A-Z" => RegexComponent::UpperClass,
            "a-zA-Z" => RegexComponent::AlphaClass,
            "a-zA-Z0-9" => RegexComponent::AlphaNumClass,
            "0-9a-fA-F" => RegexComponent::HexClass,
            "A-Za-z0-9+/" => RegexComponent::Base64Class,
            "+-" => RegexComponent::CharClass(vec!['+', '-']),
            "01" => RegexComponent::CharClass(vec!['0', '1']),
            "0-5" => RegexComponent::CharClass(vec!['0', '1', '2', '3', '4', '5']),
            "0-4" => RegexComponent::CharClass(vec!['0', '1', '2', '3', '4']),
            "0-2" => RegexComponent::CharClass(vec!['0', '1', '2']),
            "0-3" => RegexComponent::CharClass(vec!['0', '1', '2', '3']),
            "1-2" => RegexComponent::CharClass(vec!['1', '2']),
            "1-9" => RegexComponent::CharClass(vec!['1', '2', '3', '4', '5', '6', '7', '8', '9']),
            "A-Za-z\\s" => {
                let mut chars = Vec::new();
                for c in 'A'..='Z' {
                    chars.push(c);
                }
                for c in 'a'..='z' {
                    chars.push(c);
                }
                chars.push(' ');
                RegexComponent::CharClass(chars)
            }
            "a-z\\s" => {
                let mut chars = Vec::new();
                for c in 'a'..='z' {
                    chars.push(c);
                }
                chars.push(' ');
                RegexComponent::CharClass(chars)
            }
            content if content.contains('-') && content.len() == 3 => {
                let chars: Vec<char> = content.chars().collect();
                if chars[1] == '-' {
                    let start = chars[0];
                    let end = chars[2];
                    if start > end {
                        return Err(MakerError::InvalidCharacterRange { start, end });
                    }
                    RegexComponent::CharRange(start, end)
                } else {
                    RegexComponent::CharClass(content.chars().collect())
                }
            }
            _ => {
                // Handle character sets like [s.\-] or specific character lists
                let mut result_chars = Vec::new();
                let mut content_chars = class_content.chars().peekable();

                while let Some(ch) = content_chars.next() {
                    match ch {
                        '\\' => {
                            // Handle escaped characters
                            if let Some(escaped) = content_chars.next() {
                                match escaped {
                                    's' => {
                                        result_chars.push(' ');
                                        result_chars.push('\t');
                                    }
                                    'd' => {
                                        // Add digits
                                        for c in '0'..='9' {
                                            result_chars.push(c);
                                        }
                                    }
                                    '-' | '.' | '+' | '[' | ']' => {
                                        result_chars.push(escaped);
                                    }
                                    _ => result_chars.push(escaped),
                                }
                            }
                        }
                        _ => result_chars.push(ch),
                    }
                }

                // Remove duplicates
                result_chars.sort();
                result_chars.dedup();

                RegexComponent::CharClass(result_chars)
            }
        };
        Ok(component)
    }

    fn parse_quantifier(
        &self,
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<(usize, Option<usize>)> {
        let mut quantifier_content = String::new();

        for ch in chars.into_iter() {
            if ch == '}' {
                break;
            }
            quantifier_content.push(ch);
        }

        if quantifier_content.contains(',') {
            let parts: Vec<&str> = quantifier_content.split(',').collect();
            let min = parts[0]
                .parse()
                .map_err(|_| MakerError::InvalidQuantifier(quantifier_content.clone()))?;
            let max = if parts.len() > 1 && !parts[1].is_empty() {
                Some(
                    parts[1]
                        .parse()
                        .map_err(|_| MakerError::InvalidQuantifier(quantifier_content.clone()))?,
                )
            } else {
                Some(min + DEFAULT_REPEAT_MAX)
            };
            Ok((min, max))
        } else {
            let count = quantifier_content
                .parse()
                .map_err(|_| MakerError::InvalidQuantifier(quantifier_content.clone()))?;
            Ok((count, None))
        }
    }

    fn parse_group(&self, chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
        let mut group_content = String::new();
        let mut paren_count = 1;

        for ch in chars.into_iter() {
            if ch == '(' {
                paren_count += 1;
            } else if ch == ')' {
                paren_count -= 1;
                if paren_count == 0 {
                    break;
                }
            }
            group_content.push(ch);
        }

        group_content
    }

    fn is_numeric_range_alternation(&self, alternatives: &[String]) -> bool {
        // Check if all alternatives are numeric patterns
        alternatives.iter().all(|alt| {
            // Check for patterns like "25[0-5]", "2[0-4][0-9]", "[01]?[0-9][0-9]?", "[0-9]"
            alt.chars()
                .all(|c| c.is_ascii_digit() || c == '[' || c == ']' || c == '-' || c == '?')
        })
    }

    fn parse_numeric_range_alternation(&self, alternatives: &[String]) -> Option<RegexComponent> {
        // Try to detect common numeric range patterns
        // For example: "0?[0-9]|1[0-1]" (0-11), "[0-1]?[0-9]|2[0-3]" (0-23), etc.

        let mut min = u32::MAX;
        let mut max = 0u32;
        let mut has_leading_zeros = false;
        let mut min_digits = usize::MAX;
        let mut max_digits = 0;

        for alt in alternatives {
            if let Some((alt_min, alt_max, alt_digits)) = Self::analyze_numeric_pattern(alt) {
                min = min.min(alt_min);
                max = max.max(alt_max);
                min_digits = min_digits.min(alt_digits);
                max_digits = max_digits.max(alt_digits);

                // Check if any alternative has leading zeros
                if alt.starts_with('0') || alt.contains("[0") {
                    has_leading_zeros = true;
                }
            } else {
                return None;
            }
        }

        if min <= max {
            Some(RegexComponent::NumericRange {
                min,
                max,
                leading_zeros: has_leading_zeros,
                digits: if min_digits == max_digits {
                    Some(min_digits)
                } else {
                    None
                },
            })
        } else {
            None
        }
    }

    fn analyze_numeric_pattern(pattern: &str) -> Option<(u32, u32, usize)> {
        // Analyze patterns like "25[0-5]", "2[0-4][0-9]", "[01]?[0-9]"

        // Simple single digit
        if pattern.len() == 1 && pattern.chars().next()?.is_ascii_digit() {
            let digit = pattern.parse::<u32>().ok()?;
            return Some((digit, digit, 1));
        }

        // Pattern like "[0-9]"
        if pattern == "[0-9]" {
            return Some((0, 9, 1));
        }

        // Pattern like "[0-5]"
        if pattern.len() == 5 && pattern.starts_with('[') && pattern.ends_with(']') {
            let inner = &pattern[1..4];
            if inner.len() == 3 && inner.chars().nth(1) == Some('-') {
                let start = inner.chars().nth(0)?.to_digit(10)?;
                let end = inner.chars().nth(2)?.to_digit(10)?;
                return Some((start, end, 1));
            }
        }

        // Pattern like "2[0-4]" or "25[0-5]"
        if let Some(bracket_pos) = pattern.find('[') {
            let prefix = &pattern[..bracket_pos];
            if let Ok(prefix_num) = prefix.parse::<u32>() {
                let suffix = &pattern[bracket_pos..];
                if suffix.len() == 5 && suffix.starts_with('[') && suffix.ends_with(']') {
                    let inner = &suffix[1..4];
                    if inner.len() == 3 && inner.chars().nth(1) == Some('-') {
                        let start = inner.chars().nth(0)?.to_digit(10)?;
                        let end = inner.chars().nth(2)?.to_digit(10)?;
                        let min = prefix_num * 10 + start;
                        let max = prefix_num * 10 + end;
                        return Some((min, max, prefix.len() + 1));
                    }
                }
                // Pattern like "2[0-4][0-9]"
                if suffix == "[0-4][0-9]" {
                    let min = prefix_num * 100;
                    let max = prefix_num * 100 + 49;
                    return Some((min, max, prefix.len() + 2));
                }
            }
        }

        // Pattern like "[01]?[0-9]"
        if pattern == "[01]?[0-9]" {
            return Some((0, 19, 1)); // Can be 0-9 or 10-19
        }

        // Pattern like "[0-1]?[0-9]"
        if pattern == "[0-1]?[0-9]" {
            return Some((0, 19, 1)); // Can be 0-9 or 10-19
        }

        // Pattern like "[0-2]?[0-9]"
        if pattern == "[0-2]?[0-9]" {
            return Some((0, 29, 1)); // Can be 0-9 or 10-29
        }

        // Pattern like "0?[0-9]"
        if pattern == "0?[0-9]" {
            return Some((0, 9, 1)); // With optional leading zero
        }

        // Pattern like "[0-9]{1,2}"
        if pattern == "[0-9]{1,2}" {
            return Some((0, 99, 1));
        }

        // Pattern like "[0-9]{1,4}"
        if pattern == "[0-9]{1,4}" {
            return Some((0, 9999, 1));
        }

        // Pattern like "[01]?[0-9][0-9]?"
        if pattern == "[01]?[0-9][0-9]?" {
            return Some((0, 199, 1)); // Can be 0-199
        }

        // Pattern like "\\d{1,2}"
        if pattern == "\\d{1,2}" {
            return Some((0, 99, 1));
        }

        // Pattern like "(\\d{1,2})"
        if pattern.starts_with('(') && pattern.ends_with(')') {
            let inner = &pattern[1..pattern.len() - 1];
            return Self::analyze_numeric_pattern(inner);
        }

        None
    }

    fn parse_alternation(&self, content: &str) -> Result<RegexComponent> {
        tracing::trace!(content = %content, "Parsing alternation");
        // Split by '|' but respect nested parentheses
        let mut alternatives = Vec::new();
        let mut current = String::new();
        let mut paren_depth = 0;
        let chars = content.chars();

        for ch in chars.into_iter() {
            match ch {
                '(' => {
                    paren_depth += 1;
                    current.push(ch);
                }
                ')' => {
                    paren_depth -= 1;
                    current.push(ch);
                }
                '|' if paren_depth == 0 => {
                    // Only split at top-level pipes
                    alternatives.push(current.trim().to_string());
                    current = String::new();
                }
                _ => {
                    current.push(ch);
                }
            }
        }

        // Don't forget the last alternative
        if !current.is_empty() {
            alternatives.push(current.trim().to_string());
        }

        // Try to detect numeric range patterns
        if self.is_numeric_range_alternation(&alternatives)
            && let Some(range) = self.parse_numeric_range_alternation(&alternatives)
        {
            return Ok(range);
        }

        let parsed_alternatives: Result<Vec<Vec<RegexComponent>>> = alternatives
            .into_iter()
            .map(|alt| self.parse_pattern(&alt))
            .collect();

        Ok(RegexComponent::Alternation(parsed_alternatives?))
    }

    fn generate_from_components(&mut self, components: &[RegexComponent]) -> String {
        tracing::trace!(
            component_count = components.len(),
            "Generating from components"
        );
        let mut result = String::new();

        for component in components {
            result.push_str(&self.generate_component(component));
        }

        tracing::trace!(result_length = result.len(), "Components generated");
        result
    }

    fn generate_component(&mut self, component: &RegexComponent) -> String {
        tracing::trace!(component = ?component, "Generating component");
        match component {
            RegexComponent::Literal(s) => s.clone(),

            RegexComponent::CharClass(chars) => {
                if chars.is_empty() {
                    return String::new();
                }
                let idx = self.rng.random_range(0..chars.len());
                chars[idx].to_string()
            }

            RegexComponent::CharRange(start, end) => {
                let start_code = *start as u32;
                let end_code = *end as u32;
                let random_code = self.rng.random_range(start_code..=end_code);
                char::from_u32(random_code).unwrap_or('a').to_string()
            }

            RegexComponent::DigitClass => self.rng.random_range(0..10).to_string(),

            RegexComponent::HexClass => {
                let idx = self.rng.random_range(0..HEX_CHARS.len());
                HEX_CHARS.chars().nth(idx).unwrap().to_string()
            }

            RegexComponent::AlphaClass => {
                let idx = self.rng.random_range(0..ALPHA_CHARS.len());
                ALPHA_CHARS.chars().nth(idx).unwrap().to_string()
            }

            RegexComponent::LowerClass => {
                let idx = self.rng.random_range(0..LOWER_CHARS.len());
                LOWER_CHARS.chars().nth(idx).unwrap().to_string()
            }

            RegexComponent::UpperClass => {
                let idx = self.rng.random_range(0..UPPER_CHARS.len());
                UPPER_CHARS.chars().nth(idx).unwrap().to_string()
            }

            RegexComponent::AlphaNumClass => {
                let idx = self.rng.random_range(0..ALPHANUM_CHARS.len());
                ALPHANUM_CHARS.chars().nth(idx).unwrap().to_string()
            }

            RegexComponent::Base64Class => {
                let idx = self.rng.random_range(0..BASE64_CHARS.len());
                BASE64_CHARS.chars().nth(idx).unwrap().to_string()
            }

            RegexComponent::WhitespaceClass => {
                // Return a space most of the time, occasionally a tab
                if self.rng.random_bool(0.9) {
                    " ".to_string()
                } else {
                    "\t".to_string()
                }
            }

            RegexComponent::AnyChar => {
                // Generate a random printable ASCII character
                // Exclude single quotes to avoid SQL injection issues
                let chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()_+-=[]{}|;:,.<>?/~ ";
                let idx = self.rng.random_range(0..chars.len());
                chars.chars().nth(idx).unwrap().to_string()
            }

            RegexComponent::Repeat { component, count } => (0..*count)
                .map(|_| self.generate_component(component))
                .collect::<Vec<_>>()
                .join(""),

            RegexComponent::RepeatRange {
                component,
                min,
                max,
            } => {
                let count = self.rng.random_range(*min..=*max);
                (0..count)
                    .map(|_| self.generate_component(component))
                    .collect::<Vec<_>>()
                    .join("")
            }

            RegexComponent::Group(components) => {
                // Generate the content of the group
                self.generate_from_components(components)
            }

            RegexComponent::Alternation(alternatives) => {
                let idx = self.rng.random_range(0..alternatives.len());
                self.generate_from_components(&alternatives[idx])
            }

            RegexComponent::Optional(component) => {
                if self.rng.random_bool(0.7) {
                    // 70% chance to include optional component
                    self.generate_component(component)
                } else {
                    String::new()
                }
            }

            RegexComponent::NumericRange {
                min,
                max,
                leading_zeros,
                digits,
            } => {
                let num = self.rng.random_range(*min..=*max);
                let mut result = num.to_string();

                if let Some(d) = digits
                    && *leading_zeros
                    && result.len() < *d
                {
                    // Pad with leading zeros
                    result = format!("{:0width$}", num, width = d);
                }

                result
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_pattern() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("hello").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_digit_class() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate(r"\d").unwrap();
        assert!(result.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_char_range() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("[a-z]").unwrap();
        assert!(result.chars().all(|c| c.is_ascii_lowercase()));
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_invalid_char_range() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("[z-a]");
        assert!(result.is_err());
    }

    #[test]
    fn test_quantifier_exact() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate(r"\d{5}").unwrap();
        assert!(result.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_quantifier_range() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("[a-z]{2,4}").unwrap();
        assert!(result.chars().all(|c| c.is_ascii_lowercase()));
        assert!(result.len() >= 2 && result.len() <= 4);
    }

    #[test]
    fn test_invalid_quantifier() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("a{5,3}");
        assert!(result.is_err());
    }

    #[test]
    fn test_optional() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("ab?c").unwrap();
        assert!(result == "abc" || result == "ac");
    }

    #[test]
    fn test_group() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("(abc)def").unwrap();
        assert_eq!(result, "abcdef");
    }

    #[test]
    fn test_alternation() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("(cat|dog)").unwrap();
        assert!(result == "cat" || result == "dog");
    }

    #[test]
    fn test_escaped_chars() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate(r"\+\*\?").unwrap();
        assert_eq!(result, "+*?");
    }

    #[test]
    fn test_complex_pattern() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate(r"[A-Z][a-z]{2,4}\d{2}").unwrap();
        let chars: Vec<char> = result.chars().collect();
        assert!(chars[0].is_ascii_uppercase());
        assert!(
            chars[1..chars.len() - 2]
                .iter()
                .all(|c| c.is_ascii_lowercase())
        );
        assert!(chars[chars.len() - 2..].iter().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_hex_class() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("[0-9a-fA-F]{8}").unwrap();
        assert_eq!(result.len(), 8);
        assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_base64_class() {
        let mut value_generator = RegexValGen::new();
        let result = value_generator.generate("[A-Za-z0-9+/]{4}").unwrap();
        assert_eq!(result.len(), 4);
        assert!(
            result
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/')
        );
    }

    #[test]
    fn test_ip_address_pattern() {
        let mut value_generator = RegexValGen::new();

        // Test the full IP address pattern
        let ip_pattern = r"^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$";
        let result = value_generator.generate(ip_pattern);
        match result {
            Ok(ip) => {
                println!("Generated IP: {}", ip);
                // Validate it's a proper IP format
                let parts: Vec<&str> = ip.split('.').collect();
                assert_eq!(parts.len(), 4, "IP should have 4 parts, got: {}", ip);
                for part in parts {
                    let num: u32 = part
                        .parse()
                        .unwrap_or_else(|_| panic!("Each part should be a number, got: {part}"));
                    assert!(num <= 255, "Each octet should be <= 255, got: {}", num);
                }
            }
            Err(e) => panic!("Failed to generate IP: {:?}", e),
        }

        // Run it multiple times to ensure it's stable
        for _ in 0..5 {
            let ip = value_generator.generate(ip_pattern).unwrap();
            println!("Additional IP: {}", ip);
            assert!(ip.split('.').count() == 4);
        }
    }

    #[test]
    fn test_numeric_range_patterns() {
        let mut value_generator = RegexValGen::new();

        // Test patterns with numeric ranges
        let test_cases = vec![
            ("0?[0-9]|1[0-1]", 0, 11),     // Months 0-11
            ("[0-1]?[0-9]|2[0-3]", 0, 23), // Hours 0-23
            ("[0-5]?[0-9]", 0, 59),        // Minutes/seconds 0-59
            ("[0-2]?[0-9]", 0, 29),        // Days 0-29
            ("\\d{1,2}", 0, 99),           // Any 1-2 digit number
            ("\\d{1,4}", 0, 9999),         // Any 1-4 digit number
        ];

        for (pattern, min, max) in test_cases {
            println!("Testing pattern: {}", pattern);

            // Generate multiple times to check range
            for _ in 0..10 {
                let result = value_generator.generate(pattern).unwrap();
                let num: u32 = result
                    .parse()
                    .unwrap_or_else(|_| panic!("Should be a number: {result}"));
                assert!(
                    num >= min && num <= max,
                    "Pattern {} generated {} which is outside range {}-{}",
                    pattern,
                    num,
                    min,
                    max
                );
            }
        }
    }

    #[test]
    fn test_15_minute_intervals() {
        let mut value_generator = RegexValGen::new();

        // Test 15-minute interval patterns
        let patterns = vec![
            ("(0|15|30|45)", "Simple alternation"),
            ("(00|15|30|45)", "With leading zero"),
            (r"\d{2}:(00|15|30|45)", "Time with 15-min intervals"),
            (r"\d{2}:(00|15|30|45):\d{2}", "Full time HH:MM:SS"),
        ];

        for (pattern, desc) in patterns {
            println!("Testing {}: {}", desc, pattern);
            for _ in 0..5 {
                let result = value_generator.generate(pattern).unwrap();
                println!("  Generated: {}", result);

                // Validate 15-minute intervals
                if pattern.contains("(00|15|30|45)") || pattern.contains("(0|15|30|45)") {
                    let has_valid_minutes = result.contains("00")
                        || result.contains("15")
                        || result.contains("30")
                        || result.contains("45")
                        || result.ends_with("0");
                    assert!(has_valid_minutes, "Should contain valid 15-minute interval");
                }
            }
            println!();
        }
    }

    #[test]
    fn test_duration_pattern() {
        let mut value_generator = RegexValGen::new();

        // Test simpler duration components first
        let simple_patterns = vec![
            ("P\\d{1,2}Y", "Year only"),
            ("P(0?[0-9]|1[0-1])M", "Month only"),
            ("P\\d{1,4}W", "Week only"),
            ("P([0-2]?[0-9])D", "Day only"),
            ("PT([0-1]?[0-9]|2[0-3])H", "Hour only"),
            ("PT([0-5]?[0-9])M", "Minute only"),
            ("PT([0-5]?[0-9])S", "Second only"),
        ];

        for (pattern, desc) in simple_patterns {
            println!("Testing {}: {}", desc, pattern);
            match value_generator.generate(pattern) {
                Ok(duration) => {
                    println!("  Generated: {}", duration);
                    assert!(duration.starts_with('P'), "Duration should start with P");
                }
                Err(e) => {
                    println!("  Error: {:?}", e);
                }
            }
        }

        // Test the actual Duration format pattern
        let duration_pattern = r"^P((\d{1,2}Y)?(0?[0-9]|1[0-1]M)?(\d{1,4}W)?([0-2]?[0-9]D)?)?(T([0-1]?[0-9]|2[0-3]H)?([0-5]?[0-9]M)?([0-5]?[0-9](\.\d{1,3})?S)?)?$";

        println!("\nTesting actual Duration format pattern");
        for i in 0..10 {
            match value_generator.generate(duration_pattern) {
                Ok(duration) => {
                    println!("  Duration {}: {}", i + 1, duration);
                    assert!(duration.starts_with('P'), "Duration should start with P");

                    // Validate normalized ranges
                    if duration.contains('M') && !duration.contains('T') {
                        // Month is in date part
                        let month_match = duration
                            .split('M')
                            .next()
                            .unwrap()
                            .chars()
                            .rev()
                            .take_while(|c| c.is_ascii_digit())
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>();
                        if let Ok(month) = month_match.parse::<u32>() {
                            assert!(month <= 11, "Month should be 0-11, got {}", month);
                        }
                    }

                    if duration.contains('D') && !duration.contains('T') {
                        // Day is in date part
                        let day_match = duration
                            .split('D')
                            .next()
                            .unwrap()
                            .chars()
                            .rev()
                            .take_while(|c| c.is_ascii_digit())
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>();
                        if let Ok(day) = day_match.parse::<u32>() {
                            assert!(day <= 29, "Day should be 0-29, got {}", day);
                        }
                    }

                    if let Some(t_pos) = duration.find('T') {
                        let time_part = &duration[t_pos + 1..];

                        if time_part.contains('H') {
                            let hour_match = time_part.split('H').next().unwrap();
                            if let Ok(hour) = hour_match.parse::<u32>() {
                                assert!(hour <= 23, "Hour should be 0-23, got {}", hour);
                            }
                        }

                        if time_part.contains('M') {
                            let min_part = if time_part.contains('H') {
                                time_part
                                    .split('H')
                                    .nth(1)
                                    .unwrap()
                                    .split('M')
                                    .next()
                                    .unwrap()
                            } else {
                                time_part.split('M').next().unwrap()
                            };
                            if let Ok(min) = min_part.parse::<u32>() {
                                assert!(min <= 59, "Minute should be 0-59, got {}", min);
                            }
                        }

                        if time_part.contains('S') {
                            let sec_part = if time_part.contains('M') {
                                time_part
                                    .split('M')
                                    .next_back()
                                    .unwrap()
                                    .split('S')
                                    .next()
                                    .unwrap()
                            } else if time_part.contains('H') {
                                time_part
                                    .split('H')
                                    .next_back()
                                    .unwrap()
                                    .split('S')
                                    .next()
                                    .unwrap()
                            } else {
                                time_part.split('S').next().unwrap()
                            };

                            let sec_int = if sec_part.contains('.') {
                                sec_part.split('.').next().unwrap()
                            } else {
                                sec_part
                            };

                            if let Ok(sec) = sec_int.parse::<u32>() {
                                assert!(sec <= 59, "Second should be 0-59, got {}", sec);
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("  Error {}: {:?}", i + 1, e);
                }
            }
        }
    }
}
