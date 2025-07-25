mod convert_utils;
mod dsl_to_ir;
mod ir_to_dsl;
mod stack_opt;

use std::sync::{Arc, Mutex};

pub use dsl_to_ir::*;
pub use ir_to_dsl::*;
use polars_core::prelude::*;
use polars_utils::idx_vec::UnitVec;
use polars_utils::unitvec;
use polars_utils::vec::ConvertVec;
use recursive::recursive;
pub(crate) mod type_check;
pub(crate) mod type_coercion;

pub use dsl_to_ir::{is_regex_projection, prepare_projection};
pub(crate) use stack_opt::ConversionOptimizer;

use crate::constants::get_len_name;
use crate::prelude::*;

fn expr_irs_to_exprs(expr_irs: Vec<ExprIR>, expr_arena: &Arena<AExpr>) -> Vec<Expr> {
    expr_irs.convert_owned(|e| e.to_expr(expr_arena))
}

impl IR {
    #[recursive]
    fn into_lp<F, LPA>(
        self,
        conversion_fn: &F,
        lp_arena: &mut LPA,
        expr_arena: &Arena<AExpr>,
    ) -> DslPlan
    where
        F: Fn(Node, &mut LPA) -> IR,
    {
        let lp = self;
        let convert_to_lp = |node: Node, lp_arena: &mut LPA| {
            conversion_fn(node, lp_arena).into_lp(conversion_fn, lp_arena, expr_arena)
        };
        match lp {
            ir @ IR::Scan { .. } => {
                let IR::Scan {
                    ref sources,
                    ref file_info,
                    hive_parts: _,
                    predicate: _,
                    ref scan_type,
                    output_schema: _,
                    ref unified_scan_args,
                    id: _,
                } = ir
                else {
                    unreachable!()
                };

                let scan_type = Box::new(match &**scan_type {
                    #[cfg(feature = "csv")]
                    FileScanIR::Csv { options } => FileScanDsl::Csv {
                        options: options.clone(),
                    },
                    #[cfg(feature = "json")]
                    FileScanIR::NDJson { options } => FileScanDsl::NDJson {
                        options: options.clone(),
                    },
                    #[cfg(feature = "parquet")]
                    FileScanIR::Parquet {
                        options,
                        metadata: _,
                    } => FileScanDsl::Parquet {
                        options: options.clone(),
                    },
                    #[cfg(feature = "ipc")]
                    FileScanIR::Ipc {
                        options,
                        metadata: _,
                    } => FileScanDsl::Ipc {
                        options: options.clone(),
                    },
                    #[cfg(feature = "python")]
                    FileScanIR::PythonDataset {
                        dataset_object,
                        cached_ir: _,
                    } => FileScanDsl::PythonDataset {
                        dataset_object: dataset_object.clone(),
                    },
                    FileScanIR::Anonymous { options, function } => FileScanDsl::Anonymous {
                        options: options.clone(),
                        function: function.clone(),
                        file_info: file_info.clone(),
                    },
                });

                DslPlan::Scan {
                    sources: sources.clone(),
                    scan_type,
                    unified_scan_args: unified_scan_args.clone(),
                    cached_ir: Arc::new(Mutex::new(Some(ir))),
                }
            },
            #[cfg(feature = "python")]
            IR::PythonScan { .. } => DslPlan::PythonScan {
                options: Default::default(),
            },
            IR::Union { inputs, .. } => {
                let inputs = inputs
                    .into_iter()
                    .map(|node| convert_to_lp(node, lp_arena))
                    .collect();
                DslPlan::Union {
                    inputs,
                    args: Default::default(),
                }
            },
            IR::HConcat {
                inputs,
                schema: _,
                options,
            } => {
                let inputs = inputs
                    .into_iter()
                    .map(|node| convert_to_lp(node, lp_arena))
                    .collect();
                DslPlan::HConcat { inputs, options }
            },
            IR::Slice { input, offset, len } => {
                let lp = convert_to_lp(input, lp_arena);
                DslPlan::Slice {
                    input: Arc::new(lp),
                    offset,
                    len,
                }
            },
            IR::Filter { input, predicate } => {
                let lp = convert_to_lp(input, lp_arena);
                let predicate = predicate.to_expr(expr_arena);
                DslPlan::Filter {
                    input: Arc::new(lp),
                    predicate,
                }
            },
            IR::DataFrameScan {
                df,
                schema,
                output_schema: _,
            } => DslPlan::DataFrameScan { df, schema },
            IR::Select {
                expr,
                input,
                schema: _,
                options,
            } => {
                let i = convert_to_lp(input, lp_arena);
                let expr = expr_irs_to_exprs(expr, expr_arena);
                DslPlan::Select {
                    expr,
                    input: Arc::new(i),
                    options,
                }
            },
            IR::SimpleProjection { input, columns } => {
                let input = convert_to_lp(input, lp_arena);
                let expr = columns
                    .iter_names()
                    .map(|name| Expr::Column(name.clone()))
                    .collect::<Vec<_>>();
                DslPlan::Select {
                    expr,
                    input: Arc::new(input),
                    options: Default::default(),
                }
            },
            IR::Sort {
                input,
                by_column,
                slice,
                sort_options,
            } => {
                let input = Arc::new(convert_to_lp(input, lp_arena));
                let by_column = expr_irs_to_exprs(by_column, expr_arena);
                DslPlan::Sort {
                    input,
                    by_column,
                    slice,
                    sort_options,
                }
            },
            IR::Cache {
                input,
                id,
                cache_hits: _,
            } => {
                let input: Arc<DslPlan> = id
                    .downcast_arc()
                    .unwrap_or_else(|| Arc::new(convert_to_lp(input, lp_arena)));
                DslPlan::Cache { input }
            },
            IR::GroupBy {
                input,
                keys,
                aggs,
                schema,
                apply,
                maintain_order,
                options: dynamic_options,
            } => {
                let i = convert_to_lp(input, lp_arena);
                let keys = expr_irs_to_exprs(keys, expr_arena);
                let aggs = expr_irs_to_exprs(aggs, expr_arena);

                DslPlan::GroupBy {
                    input: Arc::new(i),
                    keys,
                    aggs,
                    apply: apply.map(|apply| (apply, schema)),
                    maintain_order,
                    options: dynamic_options,
                }
            },
            IR::Join {
                input_left,
                input_right,
                schema: _,
                left_on,
                right_on,
                options,
            } => {
                let i_l = convert_to_lp(input_left, lp_arena);
                let i_r = convert_to_lp(input_right, lp_arena);

                let left_on = expr_irs_to_exprs(left_on, expr_arena);
                let right_on = expr_irs_to_exprs(right_on, expr_arena);

                DslPlan::Join {
                    input_left: Arc::new(i_l),
                    input_right: Arc::new(i_r),
                    predicates: Default::default(),
                    left_on,
                    right_on,
                    options: Arc::new(JoinOptions::from(Arc::unwrap_or_clone(options))),
                }
            },
            IR::HStack {
                input,
                exprs,
                options,
                ..
            } => {
                let i = convert_to_lp(input, lp_arena);
                let exprs = expr_irs_to_exprs(exprs, expr_arena);

                DslPlan::HStack {
                    input: Arc::new(i),
                    exprs,
                    options,
                }
            },
            IR::Distinct { input, options } => {
                let i = convert_to_lp(input, lp_arena);
                let options = DistinctOptionsDSL {
                    subset: options.subset.map(|names| Selector::ByName {
                        names,
                        strict: true,
                    }),
                    maintain_order: options.maintain_order,
                    keep_strategy: options.keep_strategy,
                };
                DslPlan::Distinct {
                    input: Arc::new(i),
                    options,
                }
            },
            IR::MapFunction { input, function } => {
                let input = Arc::new(convert_to_lp(input, lp_arena));
                DslPlan::MapFunction {
                    input,
                    function: function.into(),
                }
            },
            IR::ExtContext {
                input, contexts, ..
            } => {
                let input = Arc::new(convert_to_lp(input, lp_arena));
                let contexts = contexts
                    .into_iter()
                    .map(|node| convert_to_lp(node, lp_arena))
                    .collect();
                DslPlan::ExtContext { input, contexts }
            },
            IR::Sink { input, payload } => {
                let input = Arc::new(convert_to_lp(input, lp_arena));
                let payload = match payload {
                    SinkTypeIR::Memory => SinkType::Memory,
                    SinkTypeIR::File(f) => SinkType::File(f),
                    SinkTypeIR::Partition(f) => SinkType::Partition(PartitionSinkType {
                        base_path: f.base_path,
                        file_path_cb: f.file_path_cb,
                        file_type: f.file_type,
                        sink_options: f.sink_options,
                        variant: match f.variant {
                            PartitionVariantIR::MaxSize(max_size) => {
                                PartitionVariant::MaxSize(max_size)
                            },
                            PartitionVariantIR::Parted {
                                key_exprs,
                                include_key,
                            } => PartitionVariant::Parted {
                                key_exprs: expr_irs_to_exprs(key_exprs, expr_arena),
                                include_key,
                            },
                            PartitionVariantIR::ByKey {
                                key_exprs,
                                include_key,
                            } => PartitionVariant::ByKey {
                                key_exprs: expr_irs_to_exprs(key_exprs, expr_arena),
                                include_key,
                            },
                        },
                        cloud_options: f.cloud_options,
                        per_partition_sort_by: f.per_partition_sort_by.map(|sort_by| {
                            sort_by
                                .into_iter()
                                .map(|s| SortColumn {
                                    expr: s.expr.to_expr(expr_arena),
                                    descending: s.descending,
                                    nulls_last: s.descending,
                                })
                                .collect()
                        }),
                        finish_callback: f.finish_callback,
                    }),
                };
                DslPlan::Sink { input, payload }
            },
            IR::SinkMultiple { inputs } => {
                let inputs = inputs
                    .into_iter()
                    .map(|node| convert_to_lp(node, lp_arena))
                    .collect();
                DslPlan::SinkMultiple { inputs }
            },
            #[cfg(feature = "merge_sorted")]
            IR::MergeSorted {
                input_left,
                input_right,
                key,
            } => {
                let input_left = Arc::new(convert_to_lp(input_left, lp_arena));
                let input_right = Arc::new(convert_to_lp(input_right, lp_arena));

                DslPlan::MergeSorted {
                    input_left,
                    input_right,
                    key,
                }
            },
            IR::Invalid => unreachable!(),
        }
    }
}
