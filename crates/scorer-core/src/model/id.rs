//! Strongly typed string identifiers.

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

string_id!(
    /// Rubric criterion id, e.g. `D`.
    CriterionId
);
string_id!(
    /// Rubric subitem (support class) id, e.g. `D1`.
    SubitemId
);
string_id!(
    /// Text span id, e.g. `S12`.
    SpanId
);
string_id!(
    /// Evidence id, e.g. `E3`.
    EvidenceId
);
