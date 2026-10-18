# Demonstrable behaviour

*Assessment of 2026-10-18, from the code, tests, notebooks, and Python extensions as they stand,
three build timings, and published prior art. Nothing described here is built. It records a concept,
the three uses it serves, the prior art behind it, and what a first demonstration has to measure.
Whatever survives a first demonstration belongs in a durable doc, and the rest goes stale.*

How the steps of the real code, run on real data, serve three uses. They find that something is
wrong, find out why, and explain how the system works. The reader is anyone working on the code or
its tests, or returning to it later.

The three uses share one capability and differ in nearly everything else. When the three are treated
as one, their rules mix, so each section below states which use it serves. Tests keep running in
Rust, for speed, and no use edits Rust from a notebook.

## Three uses

- **Discovery and validation**, called validation from here on, finds anomalies. A check asserts
  behaviour already known to be right. A drawing of the steps lets an eye catch what no check asked
  about.
- **Investigation** finds causes. It starts from an anomaly, states a hypothesis, and tries options
  cheaply until one survives.
- **Explanation and presentation**, called explanation from here on, shows how the system works. Its
  reader is the owner returning after time away, or a third party.

| | Validation | Investigation | Explanation |
| --- | --- | --- | --- |
| Question | Is anything wrong? | Why is it wrong, and what fixes it? | How does this work? |
| Reader | The person changing the code, at every commit | The person chasing one anomaly | The owner after time away, or a third party |
| Lifetime | As long as the code | Until the question is answered | As long as the code |
| Characteristic failure | A check passes on numbers nobody looked at | Its code outlives its question and starts to mislead | It drifts from the code it describes |

The uses hand work to one another. Validation raises an anomaly, and investigation takes it up. A
surviving option lands in Rust with an example and a check, which validation then runs on every
commit. Explanation takes up the same example to show the behaviour it pins.

## Why look at the steps

Each use needed the code's own steps during the Motis work, and each made do without them.

### Validation

Rail legs drawn as straight lines across the heat map were found by eye. No check failed, and no
check asked. The trains counted per poll were checked against Deutsche Bahn's published daily
counts, a check by external reference that the repository cannot repeat.

Three test suites check behaviour against real geography. Each asserts a number about geometry that
a reader of the test cannot see.

| Suite | Real data | What it asserts | What inspecting a failure takes |
| --- | --- | --- | --- |
| Area cases, in the crossings crate | The store's water crossings and Overture rail, in four areas | Each area holds its expected count, and every crossing's rail reaches the area | Opening the area in a notebook that clips and counts again in Python |
| Device at Dresden, in the device core | The carried crossings, packed from gold | The five nearest crossings, their distances, and 20 within the radius | Repeating a notebook calculation that no notebook in the repository holds |
| Capture coverage, in the Motis crate | Four legs of a recorded Motis response | How many legs a constructed area keeps, and how many fall outside | Decoding the legs and drawing them against the areas by hand |

The area check exists three times. The Rust harness makes it, the derivation notebook makes it
through `run_cases`, and the case view makes its own count. The Dresden values are constants copied
from a notebook during the device spike. The carried crossings follow the gold version, so a new
version changes the answers, and nothing in the repository recomputes them.

### Investigation

The cause of the straight lines was investigated in Python, cheaply, and some tries were wrong:

- A crosstab of segments with a stop abroad tied the straight lines to legs leaving the country. It
  judged a stop abroad by the format of its id.
- A first straightness measure, the longest edge's share of a shape, scored short legs on straight
  track as straight. `off_track_share`, taken from the map-matching literature, replaced it.
- A first area rule kept only legs wholly within the sampled region. It would have cut legs that
  left Thuringia for elsewhere in Germany, so the rule moved to the region's country.

Wrong tries are part of investigation. The cost lay elsewhere. Each try re-derived in Python what
the poller had already decided, because the poller's decisions were not visible. The stop-abroad
crosstab then outlived its question. On the capture after the fix, it flagged 127 segments lying
wholly within Germany, until it was removed.

### Explanation

Its owner, away for a few hours, had the comparison notebook built one step at a time, to follow it
on their return. The Germany notebook narrates its capture as a chain of named steps. Its shapes
come from a second decoder of the polylines, rather than from the ingest's own. The device README
shows real values for Dresden, and no notebook in the repository holds the calculation behind them.

## The capability

A **demonstrable** behaviour is one whose code returns, for an input a person chooses, both the
output and the steps that led to it. A table, a chart, or a map shows each step in a form a person
can check. Three properties define it:

- **Real code.** The demonstration runs the function the binary, or the test, runs.
- **Chosen input.** The input is an example or a lens. An example is an input committed with the
  code, constructed in code or read from a committed file. It carries the checks a test makes on
  its steps. A lens is an area and a time window over the store.
- **Named steps.** Each step has a constant name and returns a table. The name is what a notebook
  narrates and what a snapshot records.

A test checks an expected output for a constructed input. A log reports counts. A demonstration
returns the rows of every step, for any input. The three uses draw on it differently, as set out in
[How each use draws on it](#how-each-use-draws-on-it).

## Prior art

### For validation

| Prior art | What it contributes |
| --- | --- |
| Anscombe's quartet and the Datasaurus Dozen | Datasets with the same summary statistics and different pictures |
| MapLibre's render tests and QGIS's render checker | Visual tests of maps: a rendered map compared with an expected image, with the actual image and the difference written beside it |
| Jane Street's expect tests, `expect-test`, and `insta` | Output recorded in or beside the test, approved once, and diffed on every later run |
| ApprovalTests' geo extension and terradiff | Approval of geographic output, and diffs reporting features added, removed, and modified |
| FIT, Concordion, and Specification by Example | Documents whose examples run against the real code, and stay current because they run |
| libtest-mimic, datatest-stable, and rstest's `#[files]` | Tests built from data at run time, one per case or per file, on stable Rust |
| Playwright's trace viewer | A failed test's run, recorded action by action for later inspection |

A count asserted by a test is a summary statistic, and the Datasaurus Dozen shows how much one
hides. Validation therefore needs the steps drawn as well as checked. The render tests show the
pattern for maps: an expected picture, approved by eye, then compared on every run. The area cases
already run through libtest-mimic, under nextest.

### For investigation

| Prior art | What it contributes |
| --- | --- |
| Zeller's scientific debugging | A log of hypothesis, prediction, experiment, observation, and conclusion, repeated until a hypothesis survives |
| Rerun, Valhalla's Mobility Explorer, GraphHopper's map matching, and Anteater | The steps of one run, drawn in a viewer of their own |
| Titian, dbt's column-level lineage, Datafold's data diff, Kedro-Viz, and Hamilton | Which inputs produced an output, which rows changed between two tables, and a graph drawn from the code |
| testbook, and marimo's `mo.cli_args()` | A notebook driven from outside: chosen cells run from a test, or a notebook opened at a named input |

Zeller's log gives investigation a record of what was tried, beyond the conclusion that reaches the
slice record. The visual debuggers and lineage tools show what investigation needs to see: the steps
of one run, and the rows one layer dropped.

### For explanation

| Prior art | What it contributes |
| --- | --- |
| Knuth's WEB, nbdev, and Clerk | Source made literate, ordered for its reader |
| Glamorous Toolkit, Clerk's viewers, and Livebook's Kino | Example methods that are tests and return objects for narratives, and custom views per kind of value |
| Diátaxis | Explanation as understanding-oriented writing, apart from tutorials, how-to guides, and reference |
| Victor's "Up and Down the Ladder of Abstraction" | A behaviour shown across a parameter's range and over time |
| Rule et al., Head et al., and Pimentel et al. | Notebooks kept for exploration rather than explanation, tools to recover the code behind an output, and notebooks that fail to run again |
| marimo's app mode and exports, marimohub, and molab | A notebook served with its code hidden, exported as a static page or to run in the browser, or hosted on a notebook server |

Rule et al. found one notebook in four on GitHub with no explanatory text. The analysts they
interviewed kept notebooks personal, exploratory, and messy, and shared analyses through other
media. Exploration and explanation pull a notebook apart, so the two get notebooks of their own.
Pimentel et al. found that 24.1% of 1.4 million notebooks ran and 4% reproduced their results.
marimo's dependency-ordered cells remove two of the causes they name: hidden state and out-of-order
execution.

### Across all three

| Prior art | What it contributes |
| --- | --- |
| Storybook | One story serves development, visual and interaction tests, and generated documentation |
| `inventory`, `linkme`, pyo3-stub-gen, UniFFI, and Polars plugins | Values registered by attribute and gathered across crates, and functions exposed to Python by attribute |
| pyo3-arrow, arro3, lonboard, serde_arrow, arrow_convert, and pythonize | Rust values and tables in Python without copying, and GeoArrow drawn with no conversion |
| Observable Framework's data loaders | The alternative to an extension: a program writes its output to a file at build time |
| Pyodide, pyodide-build, and pydantic-core's Pyodide wheels | Python and pyo3 extensions run in the browser, built with a toolchain matched to Pyodide |
| parquet-wasm, arrow-js-ffi, and geoarrow-wasm | Arrow and GeoArrow passed between WebAssembly and JavaScript |

Storybook and Glamorous Toolkit show one example serving every use: a state to try, a test to run,
and a page to read. None of the binding libraries lifts a test. A test takes no arguments and
returns nothing, and code marked `#[cfg(test)]` compiles into the test binary alone. Each binds a
function, so a test is reshaped into one first.

## What practice here decides

The practices this repository already holds select among the prior art.

| Practice | What it selects |
| --- | --- |
| Rust derives, Python reads | Validation and explanation compute no step in Python |
| Throwaway code is committed, then removed with its answer | An investigation notebook lives until its question is answered |
| Each query step in its own cell with a preview, and geographic work drawn on a map | The shape of every notebook, whatever its use |
| One implementation of each format | Steps cross as Arrow, through the machinery the store's extension already uses |
| Libraries over hand-rolled code | Geometry travels as GeoArrow, which lonboard draws, rather than being rebuilt in Python |
| Fixtures shared through a cargo feature, as the predictor's are | Examples compile under a feature that only tests and the demonstration builds enable |
| The area cases run through a custom harness | One harness runs every registered example as a named test |
| Tests are written first | The example the first failing test needs is the first example a notebook shows |
| Bronze is committed and silver is not | An example runs anywhere, and a lens over silver runs where the store is filled |
| Rust already runs in the browser, through a bridge that wasm-pack builds on stable Rust | The fallback route into an exported notebook, on a toolchain already installed |
| The blog hosts notebooks under `web/static/notebooks`, with their data beside them | An exported notebook, its browser build, and its lens snapshots are hosted the same way |

## The shared mechanism

### A derivation reports its steps

A derivation takes an observer and hands it each step by name as it computes it. The binary passes
an observer that keeps nothing. A demonstration passes one that keeps every step as an Arrow table,
and returns the tables by name. Both run one function, so a notebook cannot show a step the binary
skipped or ordered differently.

Not every demonstration needs an observer. Where the public output already holds the intermediate
rows, the demonstration returns them directly. The predictor's predictions hold every crossing
within the radius, with its distance. The capture's area rule decides one leg at a time, so its
demonstration applies the rule to each leg and keeps every decision.

Steps convert through `serde_arrow`, and geometry through `geoarrow-array`. Both are in the
workspace, at the Arrow generation that SedonaDB pins. The current release of `pyo3-geoarrow`, 0.9,
builds against a later generation. It is not needed: a GeoArrow column is an Arrow column with
extension metadata, and `pyo3-arrow` passes it unchanged.

The alternative is a set of public step functions, which the binary composes and the notebook
composes again. It needs no observer, but the two compositions can drift apart, which is
explanation's characteristic failure.

### Examples, registered once

A test lifts into a notebook once it is reshaped into three parts:

| Part | What it is |
| --- | --- |
| Example | A named input: constructed in code, read from a committed file, or selected from the store by a lens |
| Demonstration | The real code run on the example, returning its named steps |
| Check | The test's assertions, made on those steps, each reporting its outcome |

The test then becomes the check run on the demonstration of the example. Each crate lists its
examples in a registry. A family read from one file, such as the area cases, is listed once and
yields one example per case. One libtest-mimic harness per crate runs every registered example as a
named test, under nextest. Examples that need a filled store run under the geo profile, as the area
cases do now.

The extension reads the same registry. It lists the examples with what each needs, and runs one by
name, returning its steps and the outcome of each check. An example that needs the store takes its
tables from the caller: the harness reads them in Rust, and the notebook through the store's
extension. A failing example prints the command that opens its crate's notebook at that example. The
notebook reads the example's name through `mo.cli_args()`.

The examples compile under a cargo feature, as the predictor's fixtures do, because code marked
`#[cfg(test)]` is out of the extension's reach. A crate enables the feature on another only as a
dev-dependency, as the platform and device cores already do for the fixtures. The feature is then
off in every build except the tests and the demonstration builds, so the site's browser build never
compiles a registry. The extension enables it as an ordinary dependency, in its native and browser
builds alike.

`inventory` registers at load time, and pyo3 already uses it inside extension modules to allow
several method blocks per class. On WebAssembly, its registrations run only once the program calls
the constructors that the linker synthesises. Whether they run in a browser build of the extension
is unverified. `linkme` lists no WebAssembly support at all.

### How much meta-programming

Three levels of mechanism register an example, each removing more repetition than the one before:

| Level | Mechanism | What it adds |
| --- | --- | --- |
| A list | A function in each crate returning its examples | Nothing to depend on, and a list kept in step by hand |
| An attribute | `inventory::submit!`, or linkme's `#[distributed_slice]` | Registration beside each example, gathered from every crate |
| A bespoke macro | An `#[example]` attribute of the repository's own | The example's source text recorded for the notebook, and a plain test generated per example |

The list is the place to start, since it works in every build, the browser's included. The attribute
level follows once a spike shows `inventory`'s registrations running in the browser build. `linkme`
stays out, since it lists no WebAssembly support. A bespoke macro follows pyo3-stub-gen's pattern, a
proc macro that emits an `inventory` submission. Its distinct gain is the source text: explanation
then shows the code that built an example beside the steps it produced. It is worth its upkeep once
registration proves repetitive, or once that source text is wanted.

### Where the extension lives

Rebuild time decides it. When a file named in an extension's cache keys changes, uv rebuilds the
extension, and the next notebook to start waits for the build. Two incremental release builds, timed
on one development machine:

| Extension | Depends on | After a change to | Rebuild |
| --- | --- | --- | --- |
| The store's | The store, DataFusion, and SedonaDB | Its own source | 5 min 50 s |
| The predictor's | The predictor | The predictor's source | Under 1 s |

Only the store extension's own crate recompiled, and it took 2,260 s of CPU time across cores.

A demonstration therefore belongs in an extension that depends on the crates it shows and does not
call the store. The notebook reads its input through the store's extension and passes the tables in.
One extension holds every crate's demonstrations and examples. If its rebuild grows beyond a few
seconds, it splits into one extension per crate. The browser adds a second reason to keep the store
out of it.

The matching code is already separated this way. It takes sessions and crossings as values, and a
separate module reads and writes the store. The crate as a whole still depends on the store, and
the rebuild time of an extension depending on that crate is unmeasured.

### The lens

A lens is an area and a time window. Applied to the store, it selects each dataset's rows within the
area and the window, layer by layer, in the order of the derivation graph. Applied to a derivation,
it selects the input, and the demonstration shows the steps between two layers.

Each dataset states where its rows lie and at what instant: a geometry or a pair of coordinate
columns, and an instant column. The store's dataset registry gains that statement, so the lens is
one function over the registry rather than a query written per notebook.

Rows join across layers on the keys the derivations already carry: a trip, a leg, a session, or a
crossing. A row present in one layer and absent from the next is what a lens finds. Explaining the
absence needs the step that dropped the row. The poller counts the legs it drops as outside, and
keeps none of them. The demonstration of its area rule returns each dropped leg with the decision
against it.

## Where a demonstration runs

Validation and investigation run natively, on the machine holding the store. Explanation runs
there for the owner. For a third party, it runs wherever the notebook is hosted.

| Host | Where the kernel and the Rust run | What the Rust needs | Inputs |
| --- | --- | --- | --- |
| The machine holding the store | Natively | The native extension | Examples, and lenses over the store |
| The blog | In the reader's browser, through Pyodide | A browser build of the extension | Examples, and lens snapshots in the notebook's public folder |
| A notebook server | On the server, natively | A build of the extension for the server | Examples and snapshots, or the store itself where the server holds a copy |

The crate's notebook reads only examples and lens snapshots, never the store, so the same file runs
on every host. A live lens over the store runs natively, in a notebook of its own. A lens worth
keeping is exported as a snapshot, which the crate's notebook then reads.

### In the browser, from the blog

The blog hosts notebooks under `web/static/notebooks` as static exports of their outputs, with
their data files beside them. A WebAssembly export instead runs Python in the reader's browser
through Pyodide, and its demonstrations run the Rust there. It needs no server, and it stays a set
of static files.

The code a demonstration runs then compiles to WebAssembly. Its crates cannot reach the store,
DataFusion, SedonaDB, the network, or a C library. The predictor and the platform core already
compile for the browser, as the site's bridge shows. The matching code shares a crate with the
store's reads and writes, and the capture's area rule shares one with the Motis client. Each moves
to a crate of its own.

A demonstration's boundary becomes Arrow IPC bytes. It takes its input tables as bytes, and returns
each step as bytes. Any binding is then a few lines, and pyarrow reads the bytes both natively and
in Pyodide.

Two routes carry the Rust into an exported notebook:

| Route | How | Toolchain | Build time |
| --- | --- | --- | --- |
| The extension, built for Pyodide | The same pyo3 extension, built as a Pyodide wheel and installed through micropip from the notebook's public folder | The Emscripten SDK at Pyodide's version, and the Rust toolchain that Pyodide's release names | Unmeasured |
| The bridge's route | A wasm-bindgen module, built as the site's bridge is, and called from the notebook's JavaScript | Stable Rust and wasm-pack, both already installed | For the bridge, 15.6 s from cached dependencies, and 1.2 s after a change to the predictor |

The first route is the one to start with. It keeps one binding, and the notebook runs unchanged in
both places. pydantic-core ships Pyodide wheels built this way. Its risk is the toolchain. Before
Python 3.14, Pyodide's Emscripten target needs a nightly Rust pinned to the release. Pyodide 314,
on Python 3.14, changes its binary interface at most once a year, and PyPI accepts its wheels.

A spike settles two questions: whether marimo's runtime is a Pyodide 314 release, and whether
`pyodide build` makes a wheel that it loads. If either fails, the bridge's route takes over. It
costs a second binding and some JavaScript, on a toolchain already in use.

Data reaches the browser in two forms. An example travels with the code. A lens cannot run there,
since silver is not published and the store does not compile for the browser. A native step
therefore exports a lens's input tables as a snapshot, which the notebook's public folder holds.
The blog's notebooks already carry their Arrow files this way. The steps are still computed live
from the snapshot, so a parameter varies in the browser. A marimo notebook in the browser has 2 GB
of memory, which bounds a snapshot.

### On a notebook server

A notebook server runs the kernel natively, so the extension runs as built, and no browser
constraint applies. marimo serves a notebook as a read-only app through `marimo run`. marimohub is a
self-hostable hub that stores and runs notebooks on compute of the owner's choosing. molab is
marimo's hosted service. It runs notebooks in sandboxes with 4 CPUs and 32 GB, shares them through
public but undiscoverable links, and embeds them in pages through iframes. Whether a molab sandbox
installs an extension of the repository's own is unverified.

A self-hosted server can hold a copy of the store, so an explanation there can open a live lens.
Its price is a server to run and secure.

The blog is the host to start with. It needs no server, and it extends how the blog already
publishes notebooks. A notebook server earns its upkeep once an explanation needs the store live,
or more memory than a browser gives.

## How each use draws on it

| Technique | Validation | Investigation | Explanation |
| --- | --- | --- | --- |
| Demonstration | Steps drawn, so an eye catches what no check asked about | The baseline that every option is compared against | The narrative's content, so the story is the code's own |
| Registered example | Run as a Rust test, with its checks, on every commit | The failing input, taken up by the investigation's notebook | A stable input that a narrative can rely on |
| Check | Asserts on the steps, in Rust | Names the failed assertion and the rows it failed on | Shows, coloured, that each claim of the narrative holds |
| Snapshot | Approved once by eye on a map, then diffed on every run | The accepted steps, drawn beside the new ones | None |
| Lens | Scans live data natively, in a notebook of its own, for rows a layer dropped | Frames the anomaly's place and time | Shows a chosen place through every layer, from an exported snapshot |
| Parameter | None | Varied without a rebuild, such as a radius or a tolerance | Swept, to show how the behaviour depends on it |
| Python in the notebook | Only an independent calculation that a check compares against | Alternatives beside the steps, never inside them | Only an independent calculation that a check compares against |
| Browser build | Runs every example's checks, as the native build does | None | Runs the exported notebook's demonstrations live |
| Notebook | The crate's notebook, opened at a failing example or a new snapshot | A notebook of its own, named for its question | The crate's notebook, read from the top, on any host |

Validation and explanation share one notebook per crate, since they share three rules. Both run real
code alone, compute no step in Python, and last as long as the code. Investigation gets notebooks of
its own, because it computes in Python and lives only until its question is answered.

### Validation

Every registered example runs as a Rust test on every commit, with its checks. Each example's steps
are also snapshotted. A new snapshot is approved by eye, on a map in the crate's notebook, as a
render test's expected image is. From then on, a change in behaviour fails the test and shows in
review as a diff of named steps. The notebook draws the accepted steps and the new ones together.
Every example also runs against the browser build, with the same checks. A difference between the
builds then fails a test rather than a reader's page.

Run natively, a lens over a day of live data shows what no example covers. It finds the rows a layer
dropped, and the steps whose values stand out. A check against an external reference, such as a
published count, goes in the slice record with its sources. Nothing in the repository can repeat it.

Validation computes nothing in Python, except an independent calculation that a check compares
against. The Dresden check is one: it compares the device's answer with a separate distance
calculation. The demonstration supplies the device's answer, and the notebook makes the independent
one. The constants can then be recomputed after any change to the carried crossings.

### Investigation

Investigation starts where validation stopped. A failing example opens its crate's notebook at that
example, or a lens frames the anomaly's place and time. A notebook named for the question then takes
the anomaly's input.

Options are tried cheaply. The demonstration's inputs and parameters vary without a rebuild: another
example, another lens, a wider radius, a tighter tolerance. Python computes alternatives beside the
steps: a new measure over a step's table, another filter, a crosstab. Here Python computing is
permitted, because nothing it computes is persisted or presented.

The notebook keeps Zeller's log: each hypothesis, its prediction, the experiment, what was observed,
and the conclusion. The conclusion goes in the slice record with its evidence, as the off-track
measurement's did.

Investigation ends in one of two ways. A surviving option moves into Rust, test first, under a new
example and its check. A measure worth keeping is ported rather than kept beside the code. The
notebook is committed as it is used, and removed once its question is answered, with the answer in
the removing commit.

Investigation does not change a Rust step from the notebook. It shows an alternative beside the
step, and the change itself is made in the crate, under its tests. Its code does not move into a
crate's notebook.

### Explanation

The crate's notebook narrates the crate's main steps, in order, over its registered examples and a
lens chosen to show the point. It explains why the steps are as they are, and which alternatives
were rejected, rather than listing every function. A parameter swept across its range shows how the
behaviour depends on it.

It computes no step, and holds only the independent calculations its checks compare against. It
stays true in three ways: its examples run as Rust tests, their steps are snapshotted, and the
notebook itself runs headless under `just test-python`. marimo runs a notebook as a script, and
pytest collects its test cells.

The owner, after time away, reads it as written: each step in its own cell with a preview, and every
shape drawn on a map. A third party reads it on the blog, exported to WebAssembly, with its
demonstrations running live in their browser. A static HTML export of its outputs stays for
notebooks with nothing to run, as the blog's are today.

### The three suites, under each use

| Suite | Validation | Investigation | Explanation |
| --- | --- | --- | --- |
| Area cases | The count and the rail checks, per case, over the store | A failing case opened at its area, with its crossings and rail drawn | How a crossing is chosen, shown on snapshots of the four areas |
| Device at Dresden | The five nearest, their distances, and the count, against the independent calculation | Another fix, or another radius, against the carried crossings | What the device shows at Dresden, and why those crossings |
| Capture coverage | The legs kept and the legs outside, per area | Each leg's decision, drawn against the area and the country | The area rule, on the recorded legs and on a snapshot of a day's capture |

The area cases' demonstration takes its crossings and rail as tables. The derivation notebook then
runs the same check on its own output before writing it, in place of `run_cases`. The case view
draws the demonstration's steps in place of its own count, and the area check exists once.

### The ground truth, under each use

The next ground-truth task records one pass per visit and interpolates each pass's instant. It is
the first demonstration to build. Its steps, under provisional names:

| Step | Rows |
| --- | --- |
| `reach` | The crossings within the session's bounding box, grown by the radius |
| `near` | Each sample within the radius of a reachable crossing, with its distance |
| `visits` | Each run of consecutive near samples, so a session reversing over a crossing makes two |
| `bracket` | For each visit, the fixes either side of the crossing's projection onto the session's path |
| `instant` | The interpolated instant of each pass |

For validation, the reversing session that the first failing test needs is the first example the
crate registers. Its check expects two passes. For investigation, a session whose passes look wrong
opens at its lens, and a sweep of the radius shows how many passes each value finds. For
explanation, the crate's notebook draws the session's path, the crossing, the radius, each visit's
samples, and the interpolated point. It draws them on the reversing example and on a snapshot of one
recorded session.

## What a first demonstration measures

- Whether marimo's WebAssembly runtime is a Pyodide 314 release, and whether `pyodide build` makes a
  wheel of the extension that it loads.
- Whether `inventory`'s registrations run in the browser build, before the registry moves from a list
  to an attribute.
- The build time of the extension, natively and for the browser.
- The download size and first-load time of an exported notebook, and its memory against the
  browser's 2 GB.
- The time the example harnesses add to `just test-no-docker`.
- Whether the parameters a demonstration exposes cover the options an investigation wants to try.
- The lines of Rust per example, and the lines of notebook per step.
- Whether the notebook finds anything the tests miss.
- The size of each step over a country-wide lens, against the memory of the machine running it.

## Not covered

- A notebook that edits or rebuilds Rust. Investigation tries options through inputs, parameters,
  and Python beside the steps, and a change to a step is made in its crate, under its tests.
- Moving tests to Python. They stay in Rust, for speed.
- Writing to the store from a demonstration.
- Profiling a derivation's performance.

## Approaches considered and rejected

**One notebook for every use.** Investigation wants cheap, messy tries, and explanation a clean
narrative. Rule et al. found that analysts keep notebooks for exploration and share analyses through
other media. The stop-abroad crosstab shows the cost here: an investigation's tool, left in a
narrative, misled.

**Steps reimplemented in Python, for validation or explanation.** A second implementation drifts
from the first, as the area check, held three times, shows. Investigation's alternatives are
another matter: they are tries rather than stand-ins, and they leave with the question.

**Binding the tests directly.** pyo3 and UniFFI bind functions. A test takes no arguments and
returns nothing, and test-only code never reaches a library. Each test has to be reshaped first,
whatever binds it.

**Nightly custom test frameworks.** `custom_test_frameworks` collects marked items into a runner of
the project's own, which is the registry this needs. It exists on nightly Rust only.

**Recorded runs.** When a test fails, it can write its steps to files, as Playwright keeps traces.
It needs no extension, and covers every test unchanged. Investigation then gets a recording rather
than a run, and cannot vary its input. Running the example in the notebook gives both, at the cost
of the extension.

**Running the Rust tests from the notebook.** A notebook can run one test through nextest and read
what it wrote. Each run then compiles the test binary, which makes investigation's tries expensive.

**Rust run inside the notebook.** marimo runs Python only, so a Rust kernel means leaving marimo for
Jupyter. Code in a cell also sits outside the crates and their tests.

**Steps written to files by a binary.** The data-loader pattern needs no extension. The notebook
then cannot vary an input or a parameter, and the files go stale beside the code. The browser route
writes only a lens's inputs to files, and computes the steps from them live.

**The store compiled for the browser.** Its extension takes 5 min 50 s to rebuild natively, and
silver is not published. A lens reaches the browser as a snapshot instead.

**Static HTML exports for demonstrations.** They capture outputs at export, so no Rust runs and no
parameter varies. They stay for notebooks with nothing to run.

**Rerun as the viewer.** It suits watching a run over time, and stays for the predictor's replay. It
shows a run apart from any narrative.

**A Python pipeline framework.** Hamilton, Kedro, and Dagster derive the graph from Python
functions. The derivations here are Rust binaries, so a framework would describe a graph it does not
run. The idea taken from them is a graph derived from code, here from the dataset registry.

**A bespoke macro first.** Deferred rather than rejected, until registration by attribute proves
repetitive or the example's source text is wanted in the notebook.

## References

Validation:

- [Anscombe's quartet](https://en.wikipedia.org/wiki/Anscombe%27s_quartet) and the
  [Datasaurus Dozen](https://en.wikipedia.org/wiki/Datasaurus_dozen) (Matejka and Fitzmaurice, CHI
  2017) — the same statistics, different pictures
- [MapLibre render tests](https://maplibre.org/maplibre-native/docs/book/platforms/android/android-tests.html)
  and [QGIS's render checker](https://qgis.org/pyqgis/3.36/core/QgsRenderChecker.html) — visual tests
  of maps
- [What if writing tests was a joyful experience?](https://blog.janestreet.com/the-joy-of-expect-tests/)
  (Somers, 2023), [expect-test](https://docs.rs/expect-test), [insta](https://docs.rs/insta), and
  [insta's advanced features](https://insta.rs/docs/advanced/) — behaviour recorded as reviewable
  output
- [ApprovalTests geo extension](https://simple-repository.app.cern.ch/project/pytest-approvaltests-geo)
  and [terradiff](https://pkg.go.dev/github.com/MVidicek/terradiff) — approving and diffing
  geographic output
- [FIT](https://fitnesse.org/FitNesse/UserGuide/WritingAcceptanceTests/FitFramework.html),
  [Concordion](https://concordion.org/), and
  [Specification by Example](https://www.oreilly.com/library/view/specification-by-example/9781617290084/)
  (Adzic, 2011) — documents that run examples against the real code
- [libtest-mimic](https://docs.rs/libtest-mimic),
  [nextest custom harnesses](https://nexte.st/book/custom-test-harnesses),
  [datatest-stable](https://docs.rs/datatest-stable), [rstest](https://docs.rs/rstest), and
  [custom test frameworks](https://doc.rust-lang.org/unstable-book/language-features/custom-test-frameworks.html)
  — tests built from data
- [Playwright trace viewer](https://playwright.dev/docs/trace-viewer) — a failed test's run,
  recorded for inspection

Investigation:

- [Scientific debugging](https://www.st.cs.uni-saarland.de/whyprogramsfail/pdf/ScientificMethod.pdf),
  from Why Programs Fail (Zeller) — hypotheses tested and logged
- [Rerun](https://github.com/rerun-io/rerun),
  [Valhalla map matching](https://mapzen.com/blog/map-matching/),
  [GraphHopper map matching](https://github.com/graphhopper/map-matching), and
  [Anteater](https://arxiv.org/abs/1907.02872) (Faust et al., 2019) — the steps of one run, in a
  viewer of their own
- [Titian](https://github.com/UCLA-SEAL/Titian) (Interlandi et al., VLDB 2016),
  [dbt column-level lineage](https://docs.getdbt.com/docs/explore/column-level-lineage),
  [Datafold data diff](https://docs.datafold.com/data-diff/what-is-data-diff),
  [Kedro-Viz previews](https://docs.kedro.org/projects/kedro-viz/en/stable/preview_nodes/), and
  [Hamilton visualisation](https://hamilton.readthedocs.io/en/latest/concepts/visualization/) —
  which rows became which
- [testbook](https://testbook.readthedocs.io/) and
  [marimo command-line arguments](https://docs.marimo.io/api/cli_args/) — notebooks opened from
  outside

Explanation:

- [Literate Programming](https://doi.org/10.1093/comjnl/27.2.97) (Knuth, 1984),
  [nbdev](https://www.fast.ai/posts/2019-11-27-nbdev.html), and
  [Clerk](https://github.com/nextjournal/clerk) — the source made literate
- [Making Systems Explainable](https://scg.unibe.ch/assets/archive/papers/Nier22a-ExplainableSystems.pdf)
  (Nierstrasz and Gîrba, 2022),
  [The Moldable Inspector](https://scg.unibe.ch/archive/papers/Chis15a-MoldableInspector.pdf)
  (Chiş et al., 2015), [Glamorous Toolkit](https://book.gtoolkit.com/what-is-glamorous-toolkit--2tbpqa98apus4jnjk6bt9r8k8),
  [Examples by example](https://book.gtoolkit.com/examples-by-example-fnfe114i8o8iglknjt6n1c4l), and
  [Kino](https://kino.hexdocs.pm/Kino.html) — example methods, narratives, and custom views
- [Diátaxis](https://diataxis.fr/) — explanation as one of four kinds of documentation
- [Up and Down the Ladder of Abstraction](https://worrydream.com/LadderOfAbstraction/)
  (Victor, 2011) — a behaviour shown across a parameter's range
- [Exploration and Explanation in Computational Notebooks](https://par.nsf.gov/biblio/10064423)
  (Rule, Tabard, and Hollan, CHI 2018),
  [Managing Messes in Computational Notebooks](https://www.microsoft.com/en-us/research/publication/managing-messes-in-computational-notebooks/)
  (Head et al., CHI 2019), and
  [A Large-scale Study about Quality and Reproducibility of Jupyter Notebooks](https://2019.msrconf.org/details/msr-2019-papers/36/A-Large-scale-Study-about-Quality-and-Reproducibility-of-Jupyter-Notebooks)
  (Pimentel et al., MSR 2019) — what happens to notebooks
- marimo on [reactivity](https://docs.marimo.io/guides/reactivity/),
  [testing with pytest](https://docs.marimo.io/guides/testing/pytest/),
  [apps](https://docs.marimo.io/guides/apps.html),
  [exporting](https://docs.marimo.io/guides/exporting/), and
  [WebAssembly exports](https://docs.marimo.io/guides/exporting/webassembly_html/), with
  [Pyodide's packages](https://pyodide.org/en/stable/usage/loading-packages.html) — running,
  testing, and publishing a notebook

Hosting:

- [Tutorial: Rust Package](https://pyodide-build.readthedocs.io/en/stable/tutorials/rust.html),
  [Emscripten wheels at pydantic](https://pydantic.dev/articles/emscripten-wheels-pydantic),
  [PyO3 and Pyodide](https://simonwillison.net/2026/Feb/6/pyo3-pyodide-wasm/) (Willison, 2026), and
  [Pyodide 314](https://blog.pyodide.org/tags/announcement/) — pyo3 extensions built for the browser
- [marimo in WebAssembly](https://docs.marimo.io/guides/wasm/),
  [deploying marimo](https://docs.marimo.io/guides/deploying/),
  [marimohub](https://github.com/marimo-team/marimohub), and
  [Reintroducing molab](https://marimo.io/blog/reintroducing-molab) — where a notebook runs
- [parquet-wasm](https://github.com/kylebarron/parquet-wasm),
  [Prototyping GeoRust and GeoArrow in WebAssembly](https://observablehq.com/@kylebarron/prototyping-georust-geoarrow-in-webassembly),
  [Lonboard in Pyodide](https://developmentseed.org/lonboard/latest/ecosystem/pyodide/), and
  [anywidget in marimo](https://docs.marimo.io/api/inputs/anywidget/) — Arrow, maps, and JavaScript
  in a browser notebook

Across all three:

- [Storybook](https://storybook.js.org/docs) — one story for development, tests, and documentation
- [inventory](https://docs.rs/inventory), [linkme](https://docs.rs/linkme),
  [pyo3 features](https://pyo3.rs/latest/features.html),
  [pyo3-stub-gen](https://docs.rs/pyo3-stub-gen),
  [UniFFI's bindings pipeline](https://mozilla.github.io/uniffi-rs/latest/internals/bindings_ir.html),
  and [Polars expression plugins](https://docs.pola.rs/user-guide/expressions/plugins/) —
  registration by attribute
- [pyo3-arrow](https://docs.rs/pyo3-arrow), [arro3](https://github.com/kylebarron/arro3),
  [pyo3-geoarrow](https://docs.rs/pyo3-geoarrow),
  [lonboard and GeoArrow](https://developmentseed.org/lonboard/latest/ecosystem/geoarrow/),
  [serde_arrow](https://docs.rs/serde_arrow), [arrow_convert](https://docs.rs/arrow_convert), and
  [pythonize](https://github.com/davidhewitt/pythonize) — Rust values in Python
- [Observable data loaders](https://observablehq.com/blog/data-loaders-for-the-win) — files
  written at build time by a program in any language

## Videos

Validation:

- [Same Stats, Different Graphs](https://www.youtube.com/watch?v=DbJyPELmhJc) (CHI 2017) — why a
  summary statistic needs a picture
- [Llewellyn Falco: ApprovalTests](https://www.youtube.com/watch?v=fCyu2ebXA8o) — approving output
  once, then comparing every run against it
- [Snapshot Testing with Insta 0.16 in Rust](https://www.youtube.com/watch?v=rCHrMqE4JOY) — `insta`
  and its review command
- [What is Specification by Example?](https://www.youtube.com/watch?v=OZol4Gypj-w) (Gojko Adzic) —
  examples as executable, living documentation
- [Viewing Playwright traces](https://www.youtube.com/watch?v=lfxjs--9ZQs) — a test's recorded run,
  inspected after the test

Investigation:

- [The Debugging Book](https://www.youtube.com/watch?v=MNRIJsoi-Ds) (Andreas Zeller and Clare
  Sudbery, GOTO 2025) — debugging as a systematic search for causes
- [Inventing on Principle](https://www.youtube.com/watch?v=PUv66718DII) (Bret Victor) — an immediate
  connection between a change and its effect
- [Rerun: A New Open Source Tool For Visualizing Computer Vision](https://www.youtube.com/watch?v=CRUdq9ECibk)
  (OpenCV Live #94) — a pipeline's intermediate state, logged and replayed
- [Hamilton: An open source microframework for creating dataframes](https://www.youtube.com/watch?v=_XUYfwougz4)
  (Stefan Krawczyk) — a dataflow graph derived from function signatures

Explanation:

- [Knuth on Literate Programming](https://www.youtube.com/watch?v=Mr3WTR0a5SM) — the program as an
  essay for its reader
- [Moldable Development & Glamorous Toolkit in Practice](https://www.youtube.com/watch?v=5PaJsEbBARo)
  (Tudor Gîrba and Kris Jenkins, GOTO 2024) — examples, custom views, and explainable systems
- [Moldable Live Programming with Clerk](https://www.youtube.com/watch?v=3bs3QX92kYA) (Philippa
  Markovics) — a source file as a notebook, with viewers of its own
- [What nobody tells you about documentation](https://www.youtube.com/watch?v=t4vKPhjcMZg) (Daniele
  Procida, PyCon Australia 2017) — the four kinds of documentation that became Diátaxis
- [Media for Thinking the Unthinkable](https://www.youtube.com/watch?v=oUaOucZRlmE) (Bret Victor) —
  representations that show a behaviour across its parameters

Notebooks themselves:

- [I don't like notebooks](https://www.youtube.com/watch?v=7jiPeIFXb6U) (Joel Grus, JupyterCon 2018)
  and [I Like Notebooks](https://www.youtube.com/watch?v=9Q6sLbz37gk) (Jeremy Howard) — hidden state
  and out-of-order execution, and nbdev's answer to them
- [Exploration and Explanation in Computational Notebooks](https://www.youtube.com/watch?v=xVfwaG8H2DM)
  (CHI 2018) and [Managing Messes in Computational Notebooks](https://www.youtube.com/watch?v=mRT4qVwBa_g)
  (CHI 2019) — the two uses pulling a notebook apart, and recovering the code behind an output
- [marimo: an open-source reactive notebook for Python](https://www.youtube.com/watch?v=9R2cQygaoxQ)
  (Akshay Agrawal) — a notebook stored as Python and run in dependency order

Hosting:

- [Python Everywhere: The State of Python on WebAssembly](https://www.youtube.com/watch?v=HklunTc9giA)
  (Hood Chatham and Gyeongjae Choi, EuroPython 2026) — Python in the browser today
- [Pyodide: A Python distribution for the browser](https://www.youtube.com/watch?v=6u2pqQ4pC04)
  (Roman Yurchak and Hood Chatham) — how Pyodide runs Python in WebAssembly

Across all three:

- [Testing Components in Storybook 7](https://www.youtube.com/watch?v=T9uKKbWYdfU) (Yann Braga) and
  [Docs Revamp in Storybook 7](https://www.youtube.com/watch?v=JBi4Dw0Fea0) (Tom Coleman) — one
  story serving as a test and as documentation
- [How Python Harnesses Rust through PyO3](https://www.youtube.com/watch?v=UilujdubqVU) (David
  Hewitt) — Rust functions exposed to Python
- [Fast geospatial data visualization and analysis with Lonboard and GeoArrow](https://www.youtube.com/watch?v=H0ZeOGmTSnU)
  (GeoPython 2024) — GeoArrow tables drawn with no conversion in Python
