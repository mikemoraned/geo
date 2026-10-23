#![cfg(test)]

use std::collections::BTreeSet;

use arrow::datatypes::FieldRef;
use medallion::Row;

pub(crate) fn column<T: Row>(name: &str) -> FieldRef {
    medallion::fields::<T>()
        .expect("describe the rows")
        .into_iter()
        .find(|field| field.name() == name)
        .unwrap_or_else(|| panic!("{}: no column named `{name}`", T::DATASET.name))
}

pub(crate) fn column_names<T: Row>() -> BTreeSet<String> {
    medallion::fields::<T>()
        .expect("describe the rows")
        .iter()
        .map(|field| field.name().clone())
        .collect()
}
