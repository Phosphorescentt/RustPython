// sliceobject.{h,c} in CPython
// spell-checker:ignore sliceobject
use super::{PyStr, PyStrRef, PyTupleRef, PyType, PyTypeRef};
use crate::{
    class::PyClassImpl,
    convert::ToPyObject,
    function::{ArgIndex, FuncArgs, OptionalArg, PyComparisonValue},
    sliceable::SaturatedSlice,
    types::{Comparable, Constructor, PyComparisonOp, Representable},
    AsObject, Context, Py, PyObject, PyObjectRef, PyPayload, PyRef, PyResult, VirtualMachine,
};
use num_bigint::{BigInt, ToBigInt};
use num_traits::{One, Signed, Zero};

#[pyclass(module = false, name = "slice", unhashable = true)]
#[derive(Debug)]
pub struct PySlice {
    pub start: Option<PyObjectRef>,
    pub stop: PyObjectRef,
    pub step: Option<PyObjectRef>,
}

impl PyPayload for PySlice {
    fn class(vm: &VirtualMachine) -> &'static Py<PyType> {
        vm.ctx.types.slice_type
    }
}

#[pyclass(with(Comparable, Representable))]
impl PySlice {
    #[pygetset]
    fn start(&self, vm: &VirtualMachine) -> PyObjectRef {
        self.start.clone().to_pyobject(vm)
    }

    pub(crate) fn start_ref<'a>(&'a self, vm: &'a VirtualMachine) -> &'a PyObject {
        match &self.start {
            Some(v) => v,
            None => vm.ctx.none.as_object(),
        }
    }

    #[pygetset]
    pub(crate) fn stop(&self, _vm: &VirtualMachine) -> PyObjectRef {
        self.stop.clone()
    }

    #[pygetset]
    fn step(&self, vm: &VirtualMachine) -> PyObjectRef {
        self.step.clone().to_pyobject(vm)
    }

    pub(crate) fn step_ref<'a>(&'a self, vm: &'a VirtualMachine) -> &'a PyObject {
        match &self.step {
            Some(v) => v,
            None => vm.ctx.none.as_object(),
        }
    }

    pub fn to_saturated(&self, vm: &VirtualMachine) -> PyResult<SaturatedSlice> {
        SaturatedSlice::with_slice(self, vm)
    }

    #[pyslot]
    fn slot_new(cls: PyTypeRef, args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        let slice: PySlice = match args.args.len() {
            0 => {
                return Err(
                    vm.new_type_error("slice() must have at least one arguments.".to_owned())
                );
            }
            1 => {
                let stop = args.bind(vm)?;
                PySlice {
                    start: None,
                    stop,
                    step: None,
                }
            }
            _ => {
                let (start, stop, step): (PyObjectRef, PyObjectRef, OptionalArg<PyObjectRef>) =
                    args.bind(vm)?;
                PySlice {
                    start: Some(start),
                    stop,
                    step: step.into_option(),
                }
            }
        };
        slice.into_ref_with_type(vm, cls).map(Into::into)
    }

    pub(crate) fn inner_indices(
        &self,
        length: &BigInt,
        vm: &VirtualMachine,
    ) -> PyResult<(BigInt, BigInt, BigInt)> {
        // Calculate step
        let step: BigInt;
        if vm.is_none(self.step_ref(vm)) {
            step = One::one();
        } else {
            // Clone the value, not the reference.
            let this_step = self.step(vm).try_index(vm)?;
            step = this_step.as_bigint().clone();

            if step.is_zero() {
                return Err(vm.new_value_error("slice step cannot be zero.".to_owned()));
            }
        }

        // For convenience
        let backwards = step.is_negative();

        // Each end of the array
        let lower = if backwards {
            (-1_i8).to_bigint().unwrap()
        } else {
            Zero::zero()
        };

        let upper = if backwards {
            lower.clone() + length
        } else {
            length.clone()
        };

        // Calculate start
        let mut start: BigInt;
        if vm.is_none(self.start_ref(vm)) {
            // Default
            start = if backwards {
                upper.clone()
            } else {
                lower.clone()
            };
        } else {
            let this_start = self.start(vm).try_index(vm)?;
            start = this_start.as_bigint().clone();

            if start < Zero::zero() {
                // From end of array
                start += length;

                if start < lower {
                    start = lower.clone();
                }
            } else if start > upper {
                start = upper.clone();
            }
        }

        // Calculate Stop
        let mut stop: BigInt;
        if vm.is_none(&self.stop) {
            stop = if backwards { lower } else { upper };
        } else {
            let this_stop = self.stop(vm).try_index(vm)?;
            stop = this_stop.as_bigint().clone();

            if stop < Zero::zero() {
                // From end of array
                stop += length;
                if stop < lower {
                    stop = lower;
                }
            } else if stop > upper {
                stop = upper;
            }
        }

        Ok((start, stop, step))
    }

    #[pymethod]
    fn indices(&self, length: ArgIndex, vm: &VirtualMachine) -> PyResult<PyTupleRef> {
        let length = length.as_bigint();
        if length.is_negative() {
            return Err(vm.new_value_error("length should not be negative.".to_owned()));
        }
        let (start, stop, step) = self.inner_indices(length, vm)?;
        Ok(vm.new_tuple((start, stop, step)))
    }

    #[allow(clippy::type_complexity)]
    #[pymethod(magic)]
    fn reduce(
        zelf: PyRef<Self>,
    ) -> PyResult<(
        PyTypeRef,
        (Option<PyObjectRef>, PyObjectRef, Option<PyObjectRef>),
    )> {
        Ok((
            zelf.class().to_owned(),
            (zelf.start.clone(), zelf.stop.clone(), zelf.step.clone()),
        ))
    }
}

impl Comparable for PySlice {
    fn cmp(
        zelf: &crate::Py<Self>,
        other: &PyObject,
        op: PyComparisonOp,
        vm: &VirtualMachine,
    ) -> PyResult<PyComparisonValue> {
        let other = class_or_notimplemented!(Self, other);

        let ret = match op {
            PyComparisonOp::Lt | PyComparisonOp::Le => None
                .or_else(|| {
                    vm.bool_seq_lt(zelf.start_ref(vm), other.start_ref(vm))
                        .transpose()
                })
                .or_else(|| vm.bool_seq_lt(&zelf.stop, &other.stop).transpose())
                .or_else(|| {
                    vm.bool_seq_lt(zelf.step_ref(vm), other.step_ref(vm))
                        .transpose()
                })
                .unwrap_or_else(|| Ok(op == PyComparisonOp::Le))?,
            PyComparisonOp::Eq | PyComparisonOp::Ne => {
                let eq = vm.identical_or_equal(zelf.start_ref(vm), other.start_ref(vm))?
                    && vm.identical_or_equal(&zelf.stop, &other.stop)?
                    && vm.identical_or_equal(zelf.step_ref(vm), other.step_ref(vm))?;
                if op == PyComparisonOp::Ne {
                    !eq
                } else {
                    eq
                }
            }
            PyComparisonOp::Gt | PyComparisonOp::Ge => None
                .or_else(|| {
                    vm.bool_seq_gt(zelf.start_ref(vm), other.start_ref(vm))
                        .transpose()
                })
                .or_else(|| vm.bool_seq_gt(&zelf.stop, &other.stop).transpose())
                .or_else(|| {
                    vm.bool_seq_gt(zelf.step_ref(vm), other.step_ref(vm))
                        .transpose()
                })
                .unwrap_or_else(|| Ok(op == PyComparisonOp::Ge))?,
        };

        Ok(PyComparisonValue::Implemented(ret))
    }
}
impl Representable for PySlice {
    #[inline]
    fn repr(zelf: &crate::Py<Self>, vm: &VirtualMachine) -> PyResult<PyStrRef> {
        let start_repr = zelf.start_ref(vm).repr(vm)?;
        let stop_repr = &zelf.stop.repr(vm)?;
        let step_repr = zelf.step_ref(vm).repr(vm)?;

        Ok(PyStr::from(format!(
            "slice({}, {}, {})",
            start_repr.as_str(),
            stop_repr.as_str(),
            step_repr.as_str()
        ))
        .into_ref(vm))
    }
}

#[pyclass(module = false, name = "EllipsisType")]
#[derive(Debug)]
pub struct PyEllipsis;

impl PyPayload for PyEllipsis {
    fn class(vm: &VirtualMachine) -> &'static Py<PyType> {
        vm.ctx.types.ellipsis_type
    }
}

impl Constructor for PyEllipsis {
    type Args = ();

    fn py_new(_cls: PyTypeRef, _args: Self::Args, vm: &VirtualMachine) -> PyResult {
        Ok(vm.ctx.ellipsis.clone().into())
    }
}

#[pyclass(with(Constructor, Representable))]
impl PyEllipsis {
    #[pymethod(magic)]
    fn reduce(&self) -> String {
        "Ellipsis".to_owned()
    }
}

impl Representable for PyEllipsis {
    #[inline]
    fn repr(_zelf: &crate::Py<Self>, vm: &VirtualMachine) -> PyResult<PyStrRef> {
        Ok(PyStr::from("Ellipsis").into_ref(vm))
    }
}

pub fn init(ctx: &Context) {
    PySlice::extend_class(ctx, ctx.types.slice_type);
    PyEllipsis::extend_class(ctx, ctx.types.ellipsis_type);
}
