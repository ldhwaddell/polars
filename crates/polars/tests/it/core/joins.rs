use polars_core::utils::{accumulate_dataframes_vertical, split_df};

use super::*;

#[test]
fn test_chunked_left_join() -> PolarsResult<()> {
    let mut band_members = df![
        "name" => ["john", "paul", "mick", "bob"],
        "band" => ["beatles", "beatles", "stones", "wailers"],
    ]?;

    let mut band_instruments = df![
        "name" => ["john", "paul", "keith"],
        "plays" => ["guitar", "bass", "guitar"]
    ]?;

    let band_instruments =
        accumulate_dataframes_vertical(split_df(&mut band_instruments, 2, false))?;
    let band_members = accumulate_dataframes_vertical(split_df(&mut band_members, 2, false))?;
    assert_eq!(band_instruments.first_col_n_chunks(), 2);
    assert_eq!(band_members.first_col_n_chunks(), 2);

    let out = band_instruments.join(
        &band_members,
        ["name"],
        ["name"],
        JoinArgs::new(JoinType::Left),
        None,
    )?;
    let expected = df![
        "name" => ["john", "paul", "keith"],
        "plays" => ["guitar", "bass", "guitar"],
        "band" => [Some("beatles"), Some("beatles"), None],
    ]?;
    assert!(out.equals_missing(&expected));

    Ok(())
}

fn create_frames() -> (DataFrame, DataFrame) {
    let s0 = Column::new("days".into(), &[0, 1, 2]);
    let s1 = Column::new("temp".into(), &[22.1, 19.9, 7.]);
    let s2 = Column::new("rain".into(), &[0.2, 0.1, 0.3]);
    let temp = DataFrame::new(vec![s0, s1, s2]).unwrap();

    let s0 = Column::new("days".into(), &[1, 2, 3, 1]);
    let s1 = Column::new("rain".into(), &[0.1, 0.2, 0.3, 0.4]);
    let rain = DataFrame::new(vec![s0, s1]).unwrap();
    (temp, rain)
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_inner_join() {
    let (temp, rain) = create_frames();

    for i in 1..8 {
        unsafe { std::env::set_var("POLARS_MAX_THREADS", format!("{i}")) };
        let joined = temp.inner_join(&rain, ["days"], ["days"]).unwrap();

        let join_col_days = Column::new("days".into(), &[1, 2, 1]);
        let join_col_temp = Column::new("temp".into(), &[19.9, 7., 19.9]);
        let join_col_rain = Column::new("rain".into(), &[0.1, 0.3, 0.1]);
        let join_col_rain_right = Column::new("rain_right".into(), [0.1, 0.2, 0.4].as_ref());
        let true_df = DataFrame::new(vec![
            join_col_days,
            join_col_temp,
            join_col_rain,
            join_col_rain_right,
        ])
        .unwrap();

        assert!(joined.equals(&true_df));
    }
}

#[test]
#[allow(clippy::float_cmp)]
#[cfg_attr(miri, ignore)]
fn test_left_join() {
    for i in 1..8 {
        unsafe { std::env::set_var("POLARS_MAX_THREADS", format!("{i}")) };
        let s0 = Column::new("days".into(), &[0, 1, 2, 3, 4]);
        let s1 = Column::new("temp".into(), &[22.1, 19.9, 7., 2., 3.]);
        let temp = DataFrame::new(vec![s0, s1]).unwrap();

        let s0 = Column::new("days".into(), &[1, 2]);
        let s1 = Column::new("rain".into(), &[0.1, 0.2]);
        let rain = DataFrame::new(vec![s0, s1]).unwrap();
        let joined = temp.left_join(&rain, ["days"], ["days"]).unwrap();
        assert_eq!(
            (joined
                .column("rain")
                .unwrap()
                .as_materialized_series()
                .sum::<f32>()
                .unwrap()
                * 10.)
                .round(),
            3.
        );
        assert_eq!(joined.column("rain").unwrap().null_count(), 3);

        // test join on string
        let s0 = Column::new("days".into(), &["mo", "tue", "wed", "thu", "fri"]);
        let s1 = Column::new("temp".into(), &[22.1, 19.9, 7., 2., 3.]);
        let temp = DataFrame::new(vec![s0, s1]).unwrap();

        let s0 = Column::new("days".into(), &["tue", "wed"]);
        let s1 = Column::new("rain".into(), &[0.1, 0.2]);
        let rain = DataFrame::new(vec![s0, s1]).unwrap();
        let joined = temp.left_join(&rain, ["days"], ["days"]).unwrap();
        assert_eq!(
            (joined
                .column("rain")
                .unwrap()
                .as_materialized_series()
                .sum::<f32>()
                .unwrap()
                * 10.)
                .round(),
            3.
        );
        assert_eq!(joined.column("rain").unwrap().null_count(), 3);
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_full_outer_join() -> PolarsResult<()> {
    let (temp, rain) = create_frames();
    let joined = temp.join(
        &rain,
        ["days"],
        ["days"],
        JoinArgs::new(JoinType::Full).with_coalesce(JoinCoalesce::CoalesceColumns),
        None,
    )?;
    assert_eq!(joined.height(), 5);
    assert_eq!(
        joined
            .column("days")?
            .as_materialized_series()
            .sum::<i32>()
            .unwrap(),
        7
    );

    let df_left = df!(
            "a"=> ["a", "b", "a", "z"],
            "b"=>[1, 2, 3, 4],
            "c"=>[6, 5, 4, 3]
    )?;
    let df_right = df!(
            "a"=> ["b", "c", "b", "a"],
            "k"=> [0, 3, 9, 6],
            "c"=> [1, 0, 2, 1]
    )?;

    let out = df_left.join(
        &df_right,
        ["a"],
        ["a"],
        JoinArgs::new(JoinType::Full).with_coalesce(JoinCoalesce::CoalesceColumns),
        None,
    )?;
    assert_eq!(out.column("c_right")?.null_count(), 1);

    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_join_with_nulls() {
    let dts = &[20, 21, 22, 23, 24, 25, 27, 28];
    let vals = &[1.2, 2.4, 4.67, 5.8, 4.4, 3.6, 7.6, 6.5];
    let df = DataFrame::new(vec![
        Column::new("date".into(), dts),
        Column::new("val".into(), vals),
    ])
    .unwrap();

    let vals2 = &[Some(1.1), None, Some(3.3), None, None];
    let df2 = DataFrame::new(vec![
        Column::new("date".into(), &dts[3..]),
        Column::new("val2".into(), vals2),
    ])
    .unwrap();

    let joined = df.left_join(&df2, ["date"], ["date"]).unwrap();
    assert_eq!(
        joined
            .column("val2")
            .unwrap()
            .f64()
            .unwrap()
            .get(joined.height() - 1),
        None
    );
}

fn get_dfs() -> (DataFrame, DataFrame) {
    let df_a = df! {
        "a" => &[1, 2, 1, 1],
        "b" => &["a", "b", "c", "c"],
        "c" => &[0, 1, 2, 3]
    }
    .unwrap();

    let df_b = df! {
        "foo" => &[1, 1, 1],
        "bar" => &["a", "c", "c"],
        "ham" => &["let", "var", "const"]
    }
    .unwrap();
    (df_a, df_b)
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_join_multiple_columns() {
    let (mut df_a, mut df_b) = get_dfs();

    // First do a hack with concatenated string dummy column
    let mut s = df_a
        .column("a")
        .unwrap()
        .cast(&DataType::String)
        .unwrap()
        .str()
        .unwrap()
        + df_a.column("b").unwrap().str().unwrap();
    s.rename("dummy".into());

    df_a.with_column(s).unwrap();
    let mut s = df_b
        .column("foo")
        .unwrap()
        .cast(&DataType::String)
        .unwrap()
        .str()
        .unwrap()
        + df_b.column("bar").unwrap().str().unwrap();
    s.rename("dummy".into());
    df_b.with_column(s).unwrap();

    let joined = df_a.left_join(&df_b, ["dummy"], ["dummy"]).unwrap();
    let ham_col = joined.column("ham").unwrap();
    let ca = ham_col.str().unwrap();

    let correct_ham = &[
        Some("let"),
        None,
        Some("var"),
        Some("const"),
        Some("var"),
        Some("const"),
    ];

    assert_eq!(Vec::from(ca), correct_ham);

    // now check the join with multiple columns
    let joined = df_a
        .join(
            &df_b,
            ["a", "b"],
            ["foo", "bar"],
            JoinType::Left.into(),
            None,
        )
        .unwrap();
    let ca = joined.column("ham").unwrap().str().unwrap();
    assert_eq!(Vec::from(ca), correct_ham);
    let joined_inner_hack = df_a.inner_join(&df_b, ["dummy"], ["dummy"]).unwrap();
    let joined_inner = df_a
        .join(
            &df_b,
            ["a", "b"],
            ["foo", "bar"],
            JoinType::Inner.into(),
            None,
        )
        .unwrap();

    assert!(
        joined_inner_hack
            .column("ham")
            .unwrap()
            .equals_missing(joined_inner.column("ham").unwrap())
    );

    let joined_full_outer_hack = df_a.full_join(&df_b, ["dummy"], ["dummy"]).unwrap();
    let joined_full_outer = df_a
        .join(
            &df_b,
            ["a", "b"],
            ["foo", "bar"],
            JoinArgs::new(JoinType::Full).with_coalesce(JoinCoalesce::CoalesceColumns),
            None,
        )
        .unwrap();
    assert!(
        joined_full_outer_hack
            .column("ham")
            .unwrap()
            .equals_missing(joined_full_outer.column("ham").unwrap())
    );
}

#[test]
#[cfg_attr(miri, ignore)]
#[cfg(feature = "dtype-categorical")]
fn test_join_categorical() {
    let (mut df_a, mut df_b) = get_dfs();

    df_a.try_apply("b", |s| {
        s.cast(&DataType::from_categories(Categories::global()))
    })
    .unwrap();
    df_b.try_apply("bar", |s| {
        s.cast(&DataType::from_categories(Categories::global()))
    })
    .unwrap();

    let out = df_a
        .join(&df_b, ["b"], ["bar"], JoinType::Left.into(), None)
        .unwrap();
    assert_eq!(out.shape(), (6, 5));
    let correct_ham = &[
        Some("let"),
        None,
        Some("var"),
        Some("const"),
        Some("var"),
        Some("const"),
    ];
    let ham_col = out.column("ham").unwrap();
    let ca = ham_col.str().unwrap();

    assert_eq!(Vec::from(ca), correct_ham);

    // test dispatch
    for jt in [JoinType::Left, JoinType::Inner, JoinType::Full] {
        let out = df_a.join(&df_b, ["b"], ["bar"], jt.into(), None).unwrap();
        let out = out.column("b").unwrap();
        assert_eq!(
            out.dtype(),
            &DataType::from_categories(Categories::global())
        );
    }

    // Test error when joining on different categories.
    let (mut df_a, mut df_b) = get_dfs();
    df_a.try_apply("b", |s| {
        s.cast(&DataType::from_categories(Categories::global()))
    })
    .unwrap();

    df_b.try_apply("bar", |s| {
        s.cast(&DataType::from_categories(Categories::new(
            PlSmallStr::from_static("test"),
            PlSmallStr::EMPTY,
            CategoricalPhysical::U32,
        )))
    })
    .unwrap();
    let out = df_a.join(&df_b, ["b"], ["bar"], JoinType::Left.into(), None);
    assert!(out.is_err());
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_empty_df_join() -> PolarsResult<()> {
    let empty: Vec<String> = vec![];
    let empty_df = DataFrame::new(vec![
        Column::new("key".into(), &empty),
        Column::new("eval".into(), &empty),
    ])
    .unwrap();

    let df = DataFrame::new(vec![
        Column::new("key".into(), &["foo"]),
        Column::new("aval".into(), &[4]),
    ])
    .unwrap();

    let out = empty_df.inner_join(&df, ["key"], ["key"]).unwrap();
    assert_eq!(out.height(), 0);
    let out = empty_df.left_join(&df, ["key"], ["key"]).unwrap();
    assert_eq!(out.height(), 0);
    let out = empty_df.full_join(&df, ["key"], ["key"]).unwrap();
    assert_eq!(out.height(), 1);
    df.left_join(&empty_df, ["key"], ["key"])?;
    df.inner_join(&empty_df, ["key"], ["key"])?;
    df.full_join(&empty_df, ["key"], ["key"])?;

    let empty: Vec<String> = vec![];
    let _empty_df = DataFrame::new(vec![
        Column::new("key".into(), &empty),
        Column::new("eval".into(), &empty),
    ])
    .unwrap();

    let df = df![
        "key" => [1i32, 2],
        "vals" => [1, 2],
    ]?;

    // https://github.com/pola-rs/polars/issues/1824
    let empty: Vec<i32> = vec![];
    let empty_df = DataFrame::new(vec![
        Column::new("key".into(), &empty),
        Column::new("1val".into(), &empty),
        Column::new("2val".into(), &empty),
    ])?;

    let out = df.left_join(&empty_df, ["key"], ["key"])?;
    assert_eq!(out.shape(), (2, 4));

    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_unit_df_join() -> PolarsResult<()> {
    let df1 = df![
        "a" => [1],
        "b" => [2]
    ]?;

    let df2 = df![
        "a" => [1, 2, 3, 4],
        "b" => [Some(1), None, Some(3), Some(4)]
    ]?;

    let out = df1.left_join(&df2, ["a"], ["a"])?;
    let expected = df![
        "a" => [1],
        "b" => [2],
        "b_right" => [1]
    ]?;
    assert!(out.equals(&expected));
    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_join_err() -> PolarsResult<()> {
    let df1 = df![
        "a" => [1, 2],
        "b" => ["foo", "bar"]
    ]?;

    let df2 = df![
        "a" => [1, 2, 3, 4],
        "b" => [true, true, true, false]
    ]?;

    // dtypes don't match, error
    assert!(
        df1.join(
            &df2,
            vec!["a", "b"],
            vec!["a", "b"],
            JoinType::Left.into(),
            None
        )
        .is_err()
    );
    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_joins_with_duplicates() -> PolarsResult<()> {
    // test joins with duplicates in both dataframes

    let df_left = df![
        "col1" => [1, 1, 2],
        "int_col" => [1, 2, 3]
    ]
    .unwrap();

    let df_right = df![
        "join_col1" => [1, 1, 1, 1, 1, 3],
        "dbl_col" => [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
    ]
    .unwrap();

    let df_inner_join = df_left
        .inner_join(&df_right, ["col1"], ["join_col1"])
        .unwrap();

    assert_eq!(df_inner_join.height(), 10);
    assert_eq!(df_inner_join.column("col1")?.null_count(), 0);
    assert_eq!(df_inner_join.column("int_col")?.null_count(), 0);
    assert_eq!(df_inner_join.column("dbl_col")?.null_count(), 0);

    let df_left_join = df_left
        .left_join(&df_right, ["col1"], ["join_col1"])
        .unwrap();

    assert_eq!(df_left_join.height(), 11);
    assert_eq!(df_left_join.column("col1")?.null_count(), 0);
    assert_eq!(df_left_join.column("int_col")?.null_count(), 0);
    assert_eq!(df_left_join.column("dbl_col")?.null_count(), 1);

    let df_full_outer_join = df_left
        .join(
            &df_right,
            ["col1"],
            ["join_col1"],
            JoinArgs::new(JoinType::Full).with_coalesce(JoinCoalesce::CoalesceColumns),
            None,
        )
        .unwrap();

    // ensure the column names don't get swapped by the drop we do
    assert_eq!(
        df_full_outer_join.get_column_names(),
        &["col1", "int_col", "dbl_col"]
    );
    assert_eq!(df_full_outer_join.height(), 12);
    assert_eq!(df_full_outer_join.column("col1")?.null_count(), 0);
    assert_eq!(df_full_outer_join.column("int_col")?.null_count(), 1);
    assert_eq!(df_full_outer_join.column("dbl_col")?.null_count(), 1);

    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_multi_joins_with_duplicates() -> PolarsResult<()> {
    // test joins with multiple join columns and duplicates in both
    // dataframes

    let df_left = df![
        "col1" => [1, 1, 1],
        "join_col2" => ["a", "a", "b"],
        "int_col" => [1, 2, 3]
    ]
    .unwrap();

    let df_right = df![
        "join_col1" => [1, 1, 1, 1, 1, 2],
        "col2" => ["a", "a", "a", "a", "a", "c"],
        "dbl_col" => [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
    ]
    .unwrap();

    let df_inner_join = df_left
        .join(
            &df_right,
            ["col1", "join_col2"],
            ["join_col1", "col2"],
            JoinType::Inner.into(),
            None,
        )
        .unwrap();

    assert_eq!(df_inner_join.height(), 10);
    assert_eq!(df_inner_join.column("col1")?.null_count(), 0);
    assert_eq!(df_inner_join.column("join_col2")?.null_count(), 0);
    assert_eq!(df_inner_join.column("int_col")?.null_count(), 0);
    assert_eq!(df_inner_join.column("dbl_col")?.null_count(), 0);

    let df_left_join = df_left
        .join(
            &df_right,
            ["col1", "join_col2"],
            ["join_col1", "col2"],
            JoinType::Left.into(),
            None,
        )
        .unwrap();

    assert_eq!(df_left_join.height(), 11);
    assert_eq!(df_left_join.column("col1")?.null_count(), 0);
    assert_eq!(df_left_join.column("join_col2")?.null_count(), 0);
    assert_eq!(df_left_join.column("int_col")?.null_count(), 0);
    assert_eq!(df_left_join.column("dbl_col")?.null_count(), 1);

    let df_full_outer_join = df_left
        .join(
            &df_right,
            ["col1", "join_col2"],
            ["join_col1", "col2"],
            JoinArgs::new(JoinType::Full).with_coalesce(JoinCoalesce::CoalesceColumns),
            None,
        )
        .unwrap();

    assert_eq!(df_full_outer_join.height(), 12);
    assert_eq!(df_full_outer_join.column("col1")?.null_count(), 0);
    assert_eq!(df_full_outer_join.column("join_col2")?.null_count(), 0);
    assert_eq!(df_full_outer_join.column("int_col")?.null_count(), 1);
    assert_eq!(df_full_outer_join.column("dbl_col")?.null_count(), 1);

    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_join_floats() -> PolarsResult<()> {
    let df_a = df! {
        "a" => &[1.0, 2.0, 1.0, 1.0],
        "b" => &["a", "b", "c", "c"],
        "c" => &[0.0, 1.0, 2.0, 3.0]
    }?;

    let df_b = df! {
        "foo" => &[1.0, 2.0, 1.0],
        "bar" => &[1.0, 1.0, 1.0],
        "ham" => &["let", "var", "const"]
    }?;

    let out = df_a.join(
        &df_b,
        vec!["a", "c"],
        vec!["foo", "bar"],
        JoinType::Left.into(),
        None,
    )?;
    assert_eq!(
        Vec::from(out.column("ham")?.str()?),
        &[None, Some("var"), None, None]
    );

    let out = df_a.join(
        &df_b,
        vec!["a", "c"],
        vec!["foo", "bar"],
        JoinArgs::new(JoinType::Full).with_coalesce(JoinCoalesce::CoalesceColumns),
        None,
    )?;
    assert_eq!(
        out.dtypes(),
        &[
            DataType::Float64,
            DataType::String,
            DataType::Float64,
            DataType::String
        ]
    );
    Ok(())
}

#[test]
#[cfg_attr(miri, ignore)]
#[cfg(feature = "lazy")]
fn test_4_threads_bit_offset() -> PolarsResult<()> {
    // run this locally with a thread pool size of 4
    // this was an obscure bug caused by not taking the offset of a bit into account.
    let n = 8i64;
    let mut left_a = (0..n).map(Some).collect::<Int64Chunked>();
    let mut left_b = (0..n)
        .map(|i| if i % 2 == 0 { None } else { Some(0) })
        .collect::<Int64Chunked>();
    left_a.rename("a".into());
    left_b.rename("b".into());
    let left_df = DataFrame::new(vec![left_a.into_column(), left_b.into_column()])?;

    let i = 1;
    let len = 8;
    let range = i..i + len;
    let mut right_a = range.clone().map(Some).collect::<Int64Chunked>();
    let mut right_b = range
        .map(|i| if i % 3 == 0 { None } else { Some(1) })
        .collect::<Int64Chunked>();
    right_a.rename("a".into());
    right_b.rename("b".into());

    let right_df = DataFrame::new(vec![right_a.into_column(), right_b.into_column()])?;
    let out = JoinBuilder::new(left_df.lazy())
        .with(right_df.lazy())
        .on([col("a"), col("b")])
        .how(JoinType::Inner)
        .join_nulls(true)
        .finish()
        .collect()
        .unwrap();
    assert_eq!(out.shape(), (1, 2));
    Ok(())
}
