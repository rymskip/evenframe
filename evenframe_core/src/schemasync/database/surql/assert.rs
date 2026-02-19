use crate::validator::{ArrayValidator, NumberValidator, StringValidator, Validator};
use tracing::{debug, trace};

/// Generate ASSERT clause from validators
pub fn generate_assert_from_validators(validators: &[Validator], value_var: &str) -> String {
    debug!(
        "Generating ASSERT clauses from {} validators for variable: {}",
        validators.len(),
        value_var
    );
    let mut assertions = Vec::new();

    for (i, validator) in validators.iter().enumerate() {
        trace!(
            "Processing validator {} of {}: {:?}",
            i + 1,
            validators.len(),
            validator
        );
        match validator {
            // String validators
            Validator::StringValidator(sv) => {
                trace!("Processing string validator: {:?}", sv);
                match sv {
                    StringValidator::Email => {
                        assertions.push(format!("string::is_email({})", value_var))
                    }
                    StringValidator::Alpha => {
                        assertions.push(format!("string::is_alpha({})", value_var))
                    }
                    StringValidator::Alphanumeric => {
                        assertions.push(format!("string::is_alphanum({})", value_var))
                    }
                    StringValidator::Hex => {
                        assertions.push(format!("string::is_hexadecimal({})", value_var))
                    }
                    StringValidator::Ip => {
                        assertions.push(format!("string::is_ip({})", value_var))
                    }
                    StringValidator::IpV4 => {
                        assertions.push(format!("string::is_ipv4({})", value_var))
                    }
                    StringValidator::IpV6 => {
                        assertions.push(format!("string::is_ipv6({})", value_var))
                    }
                    StringValidator::Url => {
                        assertions.push(format!("string::is_url({})", value_var))
                    }
                    StringValidator::Uuid => {
                        assertions.push(format!("string::is_uuid({})", value_var))
                    }
                    StringValidator::Semver => {
                        assertions.push(format!("string::is_semver({})", value_var))
                    }
                    StringValidator::Digits => {
                        assertions.push(format!("string::is_numeric({})", value_var))
                    }
                    StringValidator::MinLength(len) => {
                        assertions.push(format!("string::len({}) >= {}", value_var, len))
                    }
                    StringValidator::MaxLength(len) => {
                        assertions.push(format!("string::len({}) <= {}", value_var, len))
                    }
                    StringValidator::Length(len) => {
                        assertions.push(format!("string::len({}) = {}", value_var, len))
                    }
                    StringValidator::NonEmpty => {
                        assertions.push(format!("string::len({}) > 0", value_var))
                    }
                    StringValidator::StartsWith(prefix) => assertions.push(format!(
                        "string::starts_with({}, \"{}\")",
                        value_var, prefix
                    )),
                    StringValidator::EndsWith(suffix) => {
                        assertions.push(format!("string::ends_with({}, \"{}\")", value_var, suffix))
                    }
                    StringValidator::Includes(substring) => assertions.push(format!(
                        "string::contains({}, \"{}\")",
                        value_var, substring
                    )),
                    StringValidator::Trimmed => {
                        assertions.push(format!("{} = string::trim({})", value_var, value_var))
                    }
                    StringValidator::Lowercased => {
                        assertions.push(format!("{} = string::lowercase({})", value_var, value_var))
                    }
                    StringValidator::Uppercased => {
                        assertions.push(format!("{} = string::uppercase({})", value_var, value_var))
                    }
                    StringValidator::RegexLiteral(format_variant) => assertions.push(format!(
                        "string::matches({}, \"{}\")",
                        value_var,
                        format_variant.to_owned().into_regex().as_str()
                    )),
                    _ => {
                        trace!(
                            "String validator {:?} has no direct SurrealDB equivalent",
                            sv
                        );
                    }
                }
            }

            // Number validators
            Validator::NumberValidator(nv) => {
                trace!("Processing number validator: {:?}", nv);
                match nv {
                    NumberValidator::GreaterThan(value) => {
                        assertions.push(format!("{} > {}", value_var, value.0))
                    }
                    NumberValidator::GreaterThanOrEqualTo(value) => {
                        assertions.push(format!("{} >= {}", value_var, value.0))
                    }
                    NumberValidator::LessThan(value) => {
                        assertions.push(format!("{} < {}", value_var, value.0))
                    }
                    NumberValidator::LessThanOrEqualTo(value) => {
                        assertions.push(format!("{} <= {}", value_var, value.0))
                    }
                    NumberValidator::Between(start, end) => assertions.push(format!(
                        "{} >= {} AND {} <= {}",
                        value_var, start.0, value_var, end.0
                    )),
                    NumberValidator::Int => {
                        assertions.push(format!("type::is_int({})", value_var))
                    }
                    NumberValidator::Positive => assertions.push(format!("{} > 0", value_var)),
                    NumberValidator::NonNegative => assertions.push(format!("{} >= 0", value_var)),
                    NumberValidator::Negative => assertions.push(format!("{} < 0", value_var)),
                    NumberValidator::NonPositive => assertions.push(format!("{} <= 0", value_var)),
                    NumberValidator::MultipleOf(value) => {
                        assertions.push(format!("{} % {} = 0", value_var, value.0))
                    }
                    _ => {
                        trace!(
                            "Number validator {:?} has no direct SurrealDB equivalent",
                            nv
                        );
                    }
                }
            }

            // Array validators
            Validator::ArrayValidator(av) => {
                trace!("Processing array validator: {:?}", av);
                match av {
                    ArrayValidator::MinItems(count) => {
                        assertions.push(format!("array::len({}) >= {}", value_var, count))
                    }
                    ArrayValidator::MaxItems(count) => {
                        assertions.push(format!("array::len({}) <= {}", value_var, count))
                    }
                    ArrayValidator::ItemsCount(count) => {
                        assertions.push(format!("array::len({}) = {}", value_var, count))
                    }
                }
            }

            // Date validators - SurrealDB doesn't have built-in date comparison functions
            // These would need custom functions or direct comparisons
            Validator::DateValidator(dv) => {
                trace!(
                    "Date validator {:?} not supported - no built-in SurrealDB functions",
                    dv
                );
            }

            // BigInt validators - SurrealDB doesn't have separate BigInt type
            Validator::BigIntValidator(biv) => {
                trace!(
                    "BigInt validator {:?} not supported - no separate BigInt type in SurrealDB",
                    biv
                );
            }

            // BigDecimal validators - SurrealDB doesn't have BigDecimal type
            Validator::BigDecimalValidator(bdv) => {
                trace!(
                    "BigDecimal validator {:?} not supported - no BigDecimal type in SurrealDB",
                    bdv
                );
            }

            // Duration validators - Would need custom functions
            Validator::DurationValidator(dv) => {
                trace!(
                    "Duration validator {:?} not supported - would need custom functions",
                    dv
                );
            }
        }
    }

    // Join all assertions with AND
    let result = if assertions.is_empty() {
        debug!(
            "No valid assertions generated from {} validators",
            validators.len()
        );
        String::new()
    } else {
        debug!(
            "Generated {} valid assertions from {} validators",
            assertions.len(),
            validators.len()
        );
        trace!("Final assertions: {:?}", assertions);
        assertions.join(" AND ")
    };

    debug!(
        "ASSERT clause generation completed, result length: {}",
        result.len()
    );
    result
}
