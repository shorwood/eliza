use eliza_mad::engine::{Doctor, ErrorKind, TraceRule, TraceSource};

// -----------------------------------------------------------------------------
// ReproducesThe1966CacmConversation: Pins the published exchange sequence.
// -----------------------------------------------------------------------------

/// Reproduces every exchange printed in the 1966 CACM article.
///
/// # Panics
///
/// Panics when compilation fails or any response differs from the transcript.
#[test]
fn reproduces_the_1966_cacm_conversation() {
    let exchanges = [
        ("MEN ARE ALL ALIKE.", "IN WHAT WAY"),
        (
            "THEY'RE ALWAYS BUGGING US ABOUT SOMETHING OR OTHER.",
            "CAN YOU THINK OF A SPECIFIC EXAMPLE",
        ),
        (
            "WELL, MY BOYFRIEND MADE ME COME HERE.",
            "YOUR BOYFRIEND MADE YOU COME HERE",
        ),
        (
            "HE SAYS I'M DEPRESSED MUCH OF THE TIME.",
            "I AM SORRY TO HEAR YOU ARE DEPRESSED",
        ),
        (
            "IT'S TRUE. I AM UNHAPPY.",
            "DO YOU THINK COMING HERE WILL HELP YOU NOT TO BE UNHAPPY",
        ),
        (
            "I NEED SOME HELP, THAT MUCH SEEMS CERTAIN.",
            "WHAT WOULD IT MEAN TO YOU IF YOU GOT SOME HELP",
        ),
        (
            "PERHAPS I COULD LEARN TO GET ALONG WITH MY MOTHER.",
            "TELL ME MORE ABOUT YOUR FAMILY",
        ),
        (
            "MY MOTHER TAKES CARE OF ME.",
            "WHO ELSE IN YOUR FAMILY TAKES CARE OF YOU",
        ),
        ("MY FATHER.", "YOUR FATHER"),
        (
            "YOU ARE LIKE MY FATHER IN SOME WAYS.",
            "WHAT RESEMBLANCE DO YOU SEE",
        ),
        (
            "YOU ARE NOT VERY AGGRESSIVE BUT I THINK YOU DON'T WANT ME TO NOTICE THAT",
            "WHAT MAKES YOU THINK I AM NOT VERY AGGRESSIVE",
        ),
        (
            "YOU DON'T ARGUE WITH ME.",
            "WHY DO YOU THINK I DON'T ARGUE WITH YOU",
        ),
        (
            "YOU ARE AFRAID OF ME.",
            "DOES IT PLEASE YOU TO BELIEVE I AM AFRAID OF YOU",
        ),
        (
            "MY FATHER IS AFRAID OF EVERYBODY.",
            "WHAT ELSE COMES TO MIND WHEN YOU THINK OF YOUR FATHER",
        ),
        (
            "BULLIES.",
            "DOES THAT HAVE ANYTHING TO DO WITH THE FACT THAT YOUR BOYFRIEND MADE YOU COME HERE",
        ),
    ];
    let doctor = Doctor::compile().unwrap();
    let mut session = doctor.session().unwrap();
    for (input, expected) in exchanges {
        let response = session.respond(input).unwrap();
        assert_eq!(response.text, expected, "input: {input}");
        if input != "BULLIES." {
            continue;
        }
        assert_eq!(response.trace.source, TraceSource::Memory);
        assert_eq!(response.trace.selected_rule, None);
    }
}

// -----------------------------------------------------------------------------
// EnforcesTheHistoricalRecordContract: Pins accepted terminal input.
// -----------------------------------------------------------------------------

/// Rejects records outside the historical terminal contract.
///
/// # Panics
///
/// Panics when setup fails or invalid input reaches the interpreter.
#[test]
fn enforces_the_historical_record_contract() {
    let doctor = Doctor::compile().unwrap();
    let mut session = doctor.session().unwrap();
    assert_eq!(
        session.respond("lowercase").unwrap_err().kind(),
        ErrorKind::Input
    );
    assert_eq!(session.respond("+").unwrap_err().kind(), ErrorKind::Input);
    assert_eq!(
        session.respond(&"A".repeat(73)).unwrap_err().kind(),
        ErrorKind::Input
    );
}

// -----------------------------------------------------------------------------
// ReportsMechanicalReasoning: Pins facts captured at native selection points.
// -----------------------------------------------------------------------------

/// Reports final linked and `PRE` rules, `NONE`, and memory without inference.
///
/// # Panics
///
/// Panics when compilation, execution, or trace capture changes.
#[test]
fn reports_mechanical_reasoning() {
    let doctor = Doctor::compile().unwrap();

    let mut direct = doctor.session().unwrap();
    let response = direct.respond("I NEED HELP").unwrap();
    assert_eq!(response.trace.normalized_input, ["YOU", "NEED", "HELP"]);
    assert_eq!(response.trace.ranked_keywords, ["I"]);
    assert_eq!(response.trace.source, TraceSource::Keyword);
    assert_eq!(
        response.trace.selected_rule,
        Some(TraceRule {
            keyword: "I".to_owned(),
            decomposition: 0,
            reassembly: 0,
        })
    );
    assert_eq!(
        response.trace.to_string(),
        "NORMALIZED INPUT: YOU NEED HELP\nRANKED KEYWORDS: I\nSELECTED RULE: I/0/0\nRESPONSE SOURCE: KEYWORD"
    );

    let mut linked = doctor.session().unwrap();
    let linked = linked.respond("MEN ARE ALL ALIKE").unwrap().trace;
    assert_eq!(linked.selected_rule.unwrap().keyword, "DIT");

    let mut pre = doctor.session().unwrap();
    let pre = pre.respond("I'M SAD").unwrap().trace;
    assert_eq!(pre.selected_rule.unwrap().keyword, "I");

    let mut none = doctor.session().unwrap();
    let none = none.respond("BANANAS").unwrap().trace;
    assert_eq!(none.source, TraceSource::None);
    assert_eq!(none.selected_rule.unwrap().keyword, "NONE");
}
