use std::fmt::Display;
use datafusion::{logical_expr::Literal, prelude::Expr, scalar::ScalarValue};
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct GersId(String);

impl GersId {
    pub fn new(id: String) -> Self {
        GersId(id)
    }
}

impl Display for GersId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Literal for GersId {
   fn lit(&self) -> Expr {
        Expr::Literal(ScalarValue::Utf8(Some(self.0.clone())))
    }
}

impl Literal for &GersId {
   fn lit(&self) -> Expr {
        Expr::Literal(ScalarValue::Utf8(Some(self.0.clone())))
    }
}
