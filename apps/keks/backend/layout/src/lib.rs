use std::collections::{hash_map::Iter, HashMap};

use nalgebra::{Point2, Scalar, Vector2};

/// a Region is a vector of Point2
/// the points are assumed to be normalised to some axis system where the minimums of the points X and Y
/// values = the zero of the axis
/// in other words, a Region snugly fits tightly as possible against the 0,0 of the axes
pub struct Region<T: Clone + Scalar> {
    points: Vec<Point2<T>>
}

impl<T: Clone + Scalar> Region<T> {
    pub fn new(points: Vec<Point2<T>>) -> Region<T> {
        Region {
            points
        }
    }
}


pub struct Regions<T: Clone + Scalar> {
    regions: HashMap<usize,Region<T>>
}

impl<T: Clone + Scalar> Regions<T> {
    pub fn new(regions: HashMap<usize,Region<T>>) -> Regions<T> {
        Regions {
            regions
        }
    }
    
    pub fn iter(&self) -> Iter<'_, usize, Region<T>> {
        self.regions.iter()
    }
}

/// a Placement places a Region somewhere in a Layout
pub struct Placement<T: Clone + Scalar> {
    translation: Vector2<T>,
    region: Region<T>
}

pub struct Layout<T: Clone + Scalar> {
    placements: HashMap<usize,Placement<T>>
}