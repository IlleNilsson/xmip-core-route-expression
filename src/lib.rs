#![forbid(unsafe_code)]

//! The expression route technology — a technology of `xmip-core-route`.
//!
//! A Subscription's filter names properties, and each property is read from
//! one source. This source evaluates a predicate over other properties:
//! `expression:<predicate>` is the predicate language of
//! `xmip-core-path-predicate` — comparisons, `exists`, `starts-with`,
//! `contains`, `and`, `or`, `not` — with the context's promoted values as the
//! paths it reads, so `expression:'Amount' > 1000 and 'Region' = "EU"` reads
//! the typed context values `Amount` and `Region` and yields one truth, `true`
//! or `false`, as text a filter compares. A key the context does not hold
//! compares as null, which is the predicate language's own rule. A predicate
//! that does not parse, or compares two kinds that do not compare, is an
//! error with the engine's own reason. ADR-0046.
//!
//! The predicate engine reads through a [`StructureReader`], and the context
//! is one here: a promoted property and a structured content field are the
//! same scalar type, so nothing is converted on the way.
//!
//! A route technology does not decide anything: it reads.

use context::MessageContext;
use message::Message;
use path::{Path, PathEngine};
use path_predicate::PredicateEngine;
use route::{Source, SourceError};
use sdk::contract::{
    ContractDescriptor, ContractError, ContractId, StructureReader, StructuredValue,
};

/// The manifest leaf and the prefix a property carries.
pub const TECHNOLOGY: &str = "expression";

/// The language the predicate is written in, and the one the engine answers.
pub const LANGUAGE: &str = "predicate";

/// Reads `expression:<predicate>` as one truth over the context.
pub struct ExpressionSource;

impl Source for ExpressionSource {
    fn technology(&self) -> &'static str {
        TECHNOLOGY
    }

    fn read(&self, message: &Message, name: &str) -> Result<Option<String>, SourceError> {
        if name.trim().is_empty() {
            return Err(SourceError::new(
                TECHNOLOGY,
                name,
                "a predicate is needed after the prefix",
            ));
        }

        let reader = ContextReader::over(message.context());
        let truth = PredicateEngine
            .read(&reader, &Path::new(LANGUAGE, name))
            .map_err(|error| SourceError::new(TECHNOLOGY, name, error.to_string()))?;

        match truth {
            Some(StructuredValue::Bool(flag)) => Ok(Some(flag.to_string())),
            None => Ok(None),
            Some(other) => Err(SourceError::new(
                TECHNOLOGY,
                name,
                format!("the predicate yielded {other:?}, not a truth"),
            )),
        }
    }
}

/// A Message's context, read as the structure a predicate's paths address.
/// Each path is a context key, and what comes back is the value under it.
pub struct ContextReader<'a> {
    descriptor: ContractDescriptor,
    context: &'a MessageContext,
}

impl<'a> ContextReader<'a> {
    /// Read `context` by key.
    #[must_use]
    pub fn over(context: &'a MessageContext) -> Self {
        Self {
            descriptor: ContractDescriptor {
                id: ContractId("context".to_string()),
                version: "1".to_string(),
                representation: "application/x-xmip-context".to_string(),
            },
            context,
        }
    }
}

impl StructureReader for ContextReader<'_> {
    fn contract(&self) -> &ContractDescriptor {
        &self.descriptor
    }

    fn read(&self, path: &str) -> Result<Option<StructuredValue>, ContractError> {
        Ok(self.context.get(path).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use context::ContextValue;
    use message::MessageTreatment;
    use route::{Predicate, Value};
    use xcore::MessageId;

    fn message() -> Message {
        let context = MessageContext::new()
            .with_value("MessageType", ContextValue::Text("Order".into()))
            .with_value("Amount", ContextValue::Integer(1500))
            .with_value("Weight", ContextValue::Decimal(2.5))
            .with_value("Urgent", ContextValue::Bool(true))
            .with_value("Customer", ContextValue::Text("ACME-0042".into()));
        Message::received(
            MessageId::new(1),
            Vec::new(),
            context,
            MessageTreatment::default(),
        )
    }

    fn read(predicate: &str) -> Result<Option<String>, SourceError> {
        ExpressionSource.read(&message(), predicate)
    }

    #[test]
    fn a_predicate_over_the_context_reads_as_true_or_false() {
        assert_eq!(
            read("'Amount' > 1000 and 'MessageType' = \"Order\"").expect("reads"),
            Some("true".into())
        );
        assert_eq!(
            read("'Weight' >= 3 or not 'Urgent' = true").expect("reads"),
            Some("false".into())
        );
        assert_eq!(
            read("starts-with('Customer', \"ACME-\") and contains('Customer', \"0042\")")
                .expect("reads"),
            Some("true".into())
        );
    }

    #[test]
    fn a_key_the_context_does_not_hold_compares_as_null() {
        assert_eq!(
            read("exists('Region')").expect("reads"),
            Some("false".into())
        );
        assert_eq!(read("'Region' = null").expect("reads"), Some("true".into()));
        assert_eq!(
            read("exists('Amount')").expect("reads"),
            Some("true".into())
        );
    }

    #[test]
    fn a_predicate_that_does_not_parse_or_compare_is_refused_with_the_engines_reason() {
        let unfinished = read("'Amount' >").expect_err("unfinished");
        assert_eq!(unfinished.technology, "expression");
        assert_eq!(unfinished.property, "'Amount' >");
        assert!(unfinished.reason.starts_with("predicate:"));

        let mismatch = read("'MessageType' > 1").expect_err("text against integer");
        assert!(mismatch.reason.contains("not comparable"));

        let empty = read("  ").expect_err("nothing");
        assert!(empty.reason.contains("predicate is needed"));
    }

    #[test]
    fn the_technology_is_expression_and_promote_reads_the_prefixed_property() {
        assert_eq!(ExpressionSource.technology(), "expression");

        let sources: [&dyn Source; 1] = [&ExpressionSource];
        let over_limit = "expression:'Amount' > 1000";
        let promoted = route::promote(&message(), &sources, &[over_limit]).expect("readable");

        assert_eq!(promoted.get(over_limit), Some("true"));
        assert!(
            Predicate::equals(over_limit, Value::Boolean(true))
                .test(&promoted)
                .passed()
        );
    }
}
