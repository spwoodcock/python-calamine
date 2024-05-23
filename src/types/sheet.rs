use std::cell::RefCell;
use std::fmt::Display;
use std::sync::Arc;

use calamine::{
    Data, Dimensions as CalamineDimensions, Error, Range, SheetType, SheetVisible, XlsbCellsReader,
    XlsxCellReader,
};
use pyo3::class::basic::CompareOp;
use pyo3::prelude::*;
use pyo3::types::PyList;

use super::CalamineError;
use crate::{CellValue, LazyCell};

#[pyclass]
pub struct Dimensions {
    /// start: (row, col)
    pub start: (u32, u32),
    /// end: (row, col)
    pub end: (u32, u32),
}

impl From<CalamineDimensions> for Dimensions {
    fn from(value: CalamineDimensions) -> Self {
        Dimensions {
            start: value.start.to_owned(),
            end: value.end.to_owned(),
        }
    }
}

#[pyclass]
#[derive(Clone, Debug, PartialEq)]
pub enum SheetTypeEnum {
    /// WorkSheet
    WorkSheet,
    /// DialogSheet
    DialogSheet,
    /// MacroSheet
    MacroSheet,
    /// ChartSheet
    ChartSheet,
    /// VBA module
    Vba,
}

impl Display for SheetTypeEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SheetTypeEnum.{:?}", self)
    }
}

impl From<SheetType> for SheetTypeEnum {
    fn from(value: SheetType) -> Self {
        match value {
            SheetType::WorkSheet => Self::WorkSheet,
            SheetType::DialogSheet => Self::DialogSheet,
            SheetType::MacroSheet => Self::MacroSheet,
            SheetType::ChartSheet => Self::ChartSheet,
            SheetType::Vba => Self::Vba,
        }
    }
}

#[pyclass]
#[derive(Clone, Debug, PartialEq)]
pub enum SheetVisibleEnum {
    /// Visible
    Visible,
    /// Hidden
    Hidden,
    /// The sheet is hidden and cannot be displayed using the user interface. It is supported only by Excel formats.
    VeryHidden,
}

impl Display for SheetVisibleEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SheetVisibleEnum.{:?}", self)
    }
}

impl From<SheetVisible> for SheetVisibleEnum {
    fn from(value: SheetVisible) -> Self {
        match value {
            SheetVisible::Visible => Self::Visible,
            SheetVisible::Hidden => Self::Hidden,
            SheetVisible::VeryHidden => Self::VeryHidden,
        }
    }
}

#[pyclass]
#[derive(Clone, PartialEq)]
pub struct SheetMetadata {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    typ: SheetTypeEnum,
    #[pyo3(get)]
    visible: SheetVisibleEnum,
}

#[pymethods]
impl SheetMetadata {
    // implementation of some methods for testing
    #[new]
    fn py_new(name: &str, typ: SheetTypeEnum, visible: SheetVisibleEnum) -> Self {
        SheetMetadata {
            name: name.to_string(),
            typ,
            visible,
        }
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!(
            "SheetMetadata(name='{}', typ={}, visible={})",
            self.name, self.typ, self.visible
        ))
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> PyObject {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }
}

impl SheetMetadata {
    pub fn new(name: String, typ: SheetType, visible: SheetVisible) -> Self {
        let typ = SheetTypeEnum::from(typ);
        let visible = SheetVisibleEnum::from(visible);
        SheetMetadata { name, typ, visible }
    }
}

#[pyclass]
pub struct CalamineSheet {
    #[pyo3(get)]
    name: String,
    range: Arc<Range<Data>>,
}

impl CalamineSheet {
    pub fn new(name: String, range: Range<Data>) -> Self {
        CalamineSheet {
            name,
            range: Arc::new(range),
        }
    }
}

#[pymethods]
impl CalamineSheet {
    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("CalamineSheet(name='{}')", self.name))
    }

    #[getter]
    fn height(&self) -> usize {
        self.range.height()
    }

    #[getter]
    fn width(&self) -> usize {
        self.range.width()
    }

    #[getter]
    fn total_height(&self) -> u32 {
        self.range.end().unwrap_or_default().0
    }

    #[getter]
    fn total_width(&self) -> u32 {
        self.range.end().unwrap_or_default().1
    }

    #[getter]
    fn start(&self) -> Option<(u32, u32)> {
        self.range.start()
    }

    #[getter]
    fn end(&self) -> Option<(u32, u32)> {
        self.range.end()
    }

    #[pyo3(signature = (skip_empty_area=true, nrows=None))]
    fn to_python(
        slf: PyRef<'_, Self>,
        skip_empty_area: bool,
        nrows: Option<u32>,
    ) -> PyResult<Bound<'_, PyList>> {
        let nrows = match nrows {
            Some(nrows) => nrows,
            None => slf.range.end().map_or(0, |end| end.0 + 1),
        };

        let range = if skip_empty_area || Some((0, 0)) == slf.range.start() {
            Arc::clone(&slf.range)
        } else if let Some(end) = slf.range.end() {
            Arc::new(slf.range.range(
                (0, 0),
                (if nrows > end.0 { end.0 } else { nrows - 1 }, end.1),
            ))
        } else {
            Arc::clone(&slf.range)
        };

        Ok(PyList::new_bound(
            slf.py(),
            range.rows().take(nrows as usize).map(|row| {
                PyList::new_bound(slf.py(), row.iter().map(<&Data as Into<CellValue>>::into))
            }),
        ))
    }
}

pub(crate) enum CalamineLazyReader {
    Xlsx(RefCell<XlsxCellReader<'static>>),
    Xlsb(RefCell<XlsbCellsReader<'static>>),
}

impl CalamineLazyReader {
    fn next_cell(&self) -> Result<Option<calamine::Cell<calamine::DataRef<'static>>>, Error> {
        match self {
            CalamineLazyReader::Xlsb(reader) => {
                reader.borrow_mut().next_cell().map_err(Error::Xlsb)
            }
            CalamineLazyReader::Xlsx(reader) => {
                reader.borrow_mut().next_cell().map_err(Error::Xlsx)
            }
        }
    }
    fn dimensions(&self) -> Dimensions {
        match self {
            CalamineLazyReader::Xlsb(reader) => reader.borrow().dimensions().into(),
            CalamineLazyReader::Xlsx(reader) => reader.borrow().dimensions().into(),
        }
    }
}

#[pyclass(unsendable)]
pub struct CalamineLazySheet {
    #[pyo3(get)]
    name: String,
    reader: CalamineLazyReader,
}

impl CalamineLazySheet {
    pub(crate) fn new(name: String, reader: CalamineLazyReader) -> Self {
        CalamineLazySheet { name, reader }
    }
}

#[pymethods]
impl CalamineLazySheet {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __next__(slf: PyRef<'_, Self>) -> PyResult<Option<LazyCell>> {
        match slf.reader.next_cell() {
            Ok(Some(v)) => Ok(Some(LazyCell::from(v))),
            Ok(None) => Ok(None),
            Err(_) => Err(CalamineError::new_err("some error")),
        }
    }
    fn dimensions(slf: PyRef<'_, Self>) -> Dimensions {
        slf.reader.dimensions()
    }
}
