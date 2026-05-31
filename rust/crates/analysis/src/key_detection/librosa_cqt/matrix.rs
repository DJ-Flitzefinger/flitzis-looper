//! Dense row-major 2D matrix (extracted from rosa).
//!
//! Convention: rows = frequency bins, cols = time frames.
//! Element at (row, col) is stored at `data[row * cols + col]`.

use std::fmt;

/// Dense 2D matrix, row-major layout.
#[derive(Clone)]
pub struct Matrix {
    data: Vec<f64>,
    rows: usize,
    cols: usize,
}

impl Matrix {
    /// Zero-filled matrix.
    pub fn zeros(rows: usize, cols: usize) -> Self {
        Self {
            data: vec![0.0; rows * cols],
            rows,
            cols,
        }
    }

    /// From flat data + shape (row-major order).
    pub fn from_vec(data: Vec<f64>, rows: usize, cols: usize) -> Self {
        assert_eq!(
            data.len(),
            rows * cols,
            "data length {} does not match shape {}x{}",
            data.len(),
            rows,
            cols
        );
        Self { data, rows, cols }
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// Element access. Panics on out-of-bounds.
    pub fn get(&self, row: usize, col: usize) -> f64 {
        self.data[row * self.cols + col]
    }

    /// Mutable element access.
    pub fn get_mut(&mut self, row: usize, col: usize) -> &mut f64 {
        &mut self.data[row * self.cols + col]
    }

    /// Raw data as a flat slice (row-major).
    pub fn as_slice(&self) -> &[f64] {
        &self.data
    }
}

// --- Element-wise operations ---

impl Matrix {
    /// Apply `f` to every element, return new Matrix.
    pub fn map(&self, f: impl Fn(f64) -> f64) -> Matrix {
        Matrix {
            data: self.data.iter().map(|&x| f(x)).collect(),
            rows: self.rows,
            cols: self.cols,
        }
    }

    /// Element-wise binary operation with another Matrix of same shape.
    pub fn zip_map(&self, other: &Matrix, f: impl Fn(f64, f64) -> f64) -> Matrix {
        Matrix {
            data: self
                .data
                .iter()
                .zip(other.data.iter())
                .map(|(&a, &b)| f(a, b))
                .collect(),
            rows: self.rows,
            cols: self.cols,
        }
    }
}

// --- Matrix multiply ---

impl Matrix {
    /// Matrix multiply: `self (m×k) @ other (k×n) → result (m×n)`.
    pub fn matmul(&self, other: &Matrix) -> Matrix {
        let m = self.rows();
        let k = self.cols();
        let n = other.cols();
        assert_eq!(
            k,
            other.rows(),
            "matmul dimension mismatch: {}x{} @ {}x{}",
            m,
            k,
            other.rows(),
            n
        );

        let mut result = vec![0.0; m * n];
        unsafe {
            matrixmultiply::dgemm(
                m,
                k,
                n,
                1.0,
                self.as_slice().as_ptr(),
                k as isize,
                1,
                other.as_slice().as_ptr(),
                n as isize,
                1,
                0.0,
                result.as_mut_ptr(),
                n as isize,
                1,
            );
        }
        Matrix::from_vec(result, m, n)
    }
}

impl fmt::Debug for Matrix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Matrix({}x{})", self.rows, self.cols)
    }
}
