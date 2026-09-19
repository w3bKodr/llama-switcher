use super::{BenchmarkCase, TestCase};
use serde_json::{json, Value};

const EASY_WEIGHT: f64 = 0.0333333333;
const MEDIUM_WEIGHT: f64 = 0.05;
const HARD_WEIGHT: f64 = 0.0833333333;

fn test(id: &str, label: &str, input: Value, expected: Value) -> TestCase {
    TestCase {
        id: id.into(),
        label: label.into(),
        input,
        expected,
    }
}

fn case(
    id: &str,
    title: &str,
    description: &str,
    difficulty: &str,
    weight: f64,
    function_name: &str,
    prompt: &str,
    solution: &str,
    tests: Vec<TestCase>,
) -> BenchmarkCase {
    BenchmarkCase {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        difficulty: difficulty.into(),
        weight,
        function_name: function_name.into(),
        prompt: format!(
            "{prompt} Return exactly one fenced javascript code block containing the function."
        ),
        reference_solution: format!("```javascript\n{solution}\n```"),
        tests,
    }
}

pub(super) fn easy_cases() -> Vec<BenchmarkCase> {
    vec![
        normalize_path_case(),
        ledger_balances_case(),
        canonical_query_case(),
        inventory_delta_case(),
        rolling_windows_case(),
    ]
}

fn normalize_path_case() -> BenchmarkCase {
    case(
        "professional-js-v2-easy-02-normalize-path",
        "Normalize a portable path",
        "Resolve separators and dot segments without using platform APIs.",
        "easy", EASY_WEIGHT, "normalizePath",
        "Implement normalizePath(path). Treat both slash types as separators. Preserve whether the path is absolute. Collapse repeated separators and '.' segments. Resolve '..' against a preceding normal segment; at an absolute root discard extra '..', while a relative path must preserve unresolved leading '..'. Return '/' for an empty absolute result and '.' for an empty relative result. Do not use Node APIs.",
        r#"function normalizePath(path) {
  const absolute = /^[\\/]/.test(path);
  const out = [];
  for (const part of path.split(/[\\/]+/)) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (out.length && out[out.length - 1] !== '..') out.pop();
      else if (!absolute) out.push('..');
    } else out.push(part);
  }
  if (absolute) return '/' + out.join('/');
  return out.join('/') || '.';
}"#,
        vec![
            test("e2-1", "absolute dots", json!("/a//b/./c/../d"), json!("/a/b/d")),
            test("e2-2", "relative parents", json!("a/b/../../c"), json!("c")),
            test("e2-3", "unresolved parents", json!("../../a/../b"), json!("../../b")),
            test("e2-4", "windows separators", json!(r"\a\b\..\c"), json!("/a/c")),
            test("e2-5", "root clamp", json!("/../../x"), json!("/x")),
            test("e2-6", "empty relative", json!("a/.."), json!(".")),
        ],
    )
}

fn ledger_balances_case() -> BenchmarkCase {
    case(
        "professional-js-v2-easy-03-ledger-balances",
        "Summarize an account ledger",
        "Aggregate signed transactions with stable filtering and ordering.",
        "easy", EASY_WEIGHT, "ledgerBalances",
        "Implement ledgerBalances(transactions). Each transaction has id, account, kind ('credit' or 'debit'), and an integer amount. Ignore later transactions whose id has already appeared. Credits add and debits subtract. Return one object per account sorted lexically as {account,balance,count}; include zero balances and count only accepted unique transactions. Do not mutate the input.",
        r#"function ledgerBalances(transactions) {
  const seen = new Set(), map = new Map();
  for (const t of transactions) {
    if (seen.has(t.id)) continue;
    seen.add(t.id);
    const row = map.get(t.account) || { account: t.account, balance: 0, count: 0 };
    row.balance += t.kind === 'credit' ? t.amount : -t.amount;
    row.count++;
    map.set(t.account, row);
  }
  return [...map.values()].sort((a,b) => a.account.localeCompare(b.account));
}"#,
        vec![
            test("e3-1", "mixed ledger", json!([{"id":"1","account":"cash","kind":"credit","amount":10},{"id":"2","account":"cash","kind":"debit","amount":4},{"id":"3","account":"bank","kind":"credit","amount":7}]), json!([{"account":"bank","balance":7,"count":1},{"account":"cash","balance":6,"count":2}])),
            test("e3-2", "duplicate id", json!([{"id":"x","account":"a","kind":"credit","amount":5},{"id":"x","account":"b","kind":"credit","amount":99}]), json!([{"account":"a","balance":5,"count":1}])),
            test("e3-3", "zero balance", json!([{"id":"1","account":"a","kind":"credit","amount":8},{"id":"2","account":"a","kind":"debit","amount":8}]), json!([{"account":"a","balance":0,"count":2}])),
            test("e3-4", "lexical accounts", json!([{"id":"1","account":"z","kind":"credit","amount":1},{"id":"2","account":"A","kind":"debit","amount":2},{"id":"3","account":"m","kind":"credit","amount":3}]), json!([{"account":"A","balance":-2,"count":1},{"account":"m","balance":3,"count":1},{"account":"z","balance":1,"count":1}])),
            test("e3-5", "empty", json!([]), json!([])),
            test("e3-6", "negative amount follows kind", json!([{"id":"1","account":"a","kind":"debit","amount":-3}]), json!([{"account":"a","balance":3,"count":1}])),
        ],
    )
}

fn canonical_query_case() -> BenchmarkCase {
    case(
        "professional-js-v2-easy-04-canonical-query",
        "Canonicalize query entries",
        "Filter, encode, sort, and serialize repeated query parameters.",
        "easy", EASY_WEIGHT, "canonicalQuery",
        "Implement canonicalQuery(input), where input is {entries, ignored}. entries is an array of [key,value] pairs; value may be a string, number, boolean, or null. Remove entries whose key occurs in ignored. Convert null to an empty string and other values with String(). Sort by key and then value using ordinary JavaScript lexical comparison, preserving original order for exact ties. Percent-encode keys and values with encodeURIComponent and join as key=value pairs using '&'. Do not mutate input.",
        r#"function canonicalQuery(input) {
  const ignored = new Set(input.ignored);
  return input.entries.map((p,i) => ({k:String(p[0]),v:p[1]===null?'':String(p[1]),i}))
    .filter(x => !ignored.has(x.k))
    .sort((a,b) => a.k < b.k ? -1 : a.k > b.k ? 1 : a.v < b.v ? -1 : a.v > b.v ? 1 : a.i-b.i)
    .map(x => encodeURIComponent(x.k)+'='+encodeURIComponent(x.v)).join('&');
}"#,
        vec![
            test("e4-1", "sort and encode", json!({"entries":[["q","red fox"],["page",2],["q","blue"]],"ignored":[]}), json!("page=2&q=blue&q=red%20fox")),
            test("e4-2", "ignored", json!({"entries":[["token","secret"],["a",1]],"ignored":["token"]}), json!("a=1")),
            test("e4-3", "null and booleans", json!({"entries":[["x",null],["ok",false]],"ignored":[]}), json!("ok=false&x=")),
            test("e4-4", "special characters", json!({"entries":[["a/b","x&y"],["a","="]],"ignored":[]}), json!("a=%3D&a%2Fb=x%26y")),
            test("e4-5", "empty", json!({"entries":[],"ignored":[]}), json!("")),
            test("e4-6", "numeric lexical values", json!({"entries":[["n",10],["n",2]],"ignored":[]}), json!("n=10&n=2")),
        ],
    )
}

fn inventory_delta_case() -> BenchmarkCase {
    case(
        "professional-js-v2-easy-05-inventory-delta",
        "Reconcile inventory snapshots",
        "Compare duplicate-bearing snapshots and report only changes.",
        "easy", EASY_WEIGHT, "inventoryDelta",
        "Implement inventoryDelta(input), where input has before and after arrays of {sku,quantity}. Sum duplicate rows within each snapshot. Return changed SKUs only, sorted lexically, as {sku,before,after,delta}. Missing SKUs count as zero. Do not mutate input.",
        r#"function inventoryDelta(input) {
  function totals(rows){const m=new Map();for(const r of rows)m.set(r.sku,(m.get(r.sku)||0)+r.quantity);return m}
  const a=totals(input.before), b=totals(input.after), keys=[...new Set([...a.keys(),...b.keys()])].sort(), out=[];
  for(const sku of keys){const before=a.get(sku)||0,after=b.get(sku)||0;if(before!==after)out.push({sku,before,after,delta:after-before})}
  return out;
}"#,
        vec![
            test("e5-1", "added removed changed", json!({"before":[{"sku":"a","quantity":2},{"sku":"b","quantity":5}],"after":[{"sku":"a","quantity":3},{"sku":"c","quantity":4}]}), json!([{"sku":"a","before":2,"after":3,"delta":1},{"sku":"b","before":5,"after":0,"delta":-5},{"sku":"c","before":0,"after":4,"delta":4}])),
            test("e5-2", "duplicates", json!({"before":[{"sku":"a","quantity":2},{"sku":"a","quantity":3}],"after":[{"sku":"a","quantity":5}]}), json!([])),
            test("e5-3", "negative quantities", json!({"before":[{"sku":"x","quantity":-2}],"after":[{"sku":"x","quantity":1}]}), json!([{"sku":"x","before":-2,"after":1,"delta":3}])),
            test("e5-4", "zero totals", json!({"before":[{"sku":"x","quantity":1}],"after":[{"sku":"x","quantity":0}]}), json!([{"sku":"x","before":1,"after":0,"delta":-1}])),
            test("e5-5", "empty", json!({"before":[],"after":[]}), json!([])),
            test("e5-6", "lexical order", json!({"before":[],"after":[{"sku":"z","quantity":1},{"sku":"A","quantity":1},{"sku":"m","quantity":1}]}), json!([{"sku":"A","before":0,"after":1,"delta":1},{"sku":"m","before":0,"after":1,"delta":1},{"sku":"z","before":0,"after":1,"delta":1}])),
        ],
    )
}

fn rolling_windows_case() -> BenchmarkCase {
    case(
        "professional-js-v2-easy-06-rolling-windows",
        "Compute rolling window statistics",
        "Produce exact moving aggregates with invalid-value handling.",
        "easy", EASY_WEIGHT, "rollingWindows",
        "Implement rollingWindows(input), where input is {values,size}. For every complete contiguous window of exactly size elements, return {start,sum,min,max}. size must be a positive integer no larger than values.length; otherwise return []. Values are finite numbers. Do not mutate input.",
        r#"function rollingWindows(input){const {values,size}=input;if(!Number.isInteger(size)||size<=0||size>values.length)return[];const out=[];for(let i=0;i+size<=values.length;i++){const w=values.slice(i,i+size);out.push({start:i,sum:w.reduce((a,b)=>a+b,0),min:Math.min(...w),max:Math.max(...w)})}return out}"#,
        vec![
            test("e6-1", "normal", json!({"values":[1,3,2,5],"size":2}), json!([{"start":0,"sum":4,"min":1,"max":3},{"start":1,"sum":5,"min":2,"max":3},{"start":2,"sum":7,"min":2,"max":5}])),
            test("e6-2", "whole array", json!({"values":[-2,4,1],"size":3}), json!([{"start":0,"sum":3,"min":-2,"max":4}])),
            test("e6-3", "singletons", json!({"values":[2,-1],"size":1}), json!([{"start":0,"sum":2,"min":2,"max":2},{"start":1,"sum":-1,"min":-1,"max":-1}])),
            test("e6-4", "zero size", json!({"values":[1,2],"size":0}), json!([])),
            test("e6-5", "oversized", json!({"values":[1,2],"size":3}), json!([])),
            test("e6-6", "empty", json!({"values":[],"size":1}), json!([])),
        ],
    )
}

pub(super) fn medium_cases() -> Vec<BenchmarkCase> {
    vec![
        deep_merge_case(),
        expression_case(),
        sessionize_case(),
        json_patch_case(),
        allocate_case(),
    ]
}

pub(super) fn hard_cases() -> Vec<BenchmarkCase> {
    vec![
        semver_case(),
        sheet_case(),
        route_case(),
        schedule_case(),
        three_way_merge_case(),
    ]
}

// Medium and hard cases follow below.

fn deep_merge_case() -> BenchmarkCase {
    case("professional-js-v2-medium-02-deep-merge", "Deep-merge configuration", "Apply recursive object merge rules with deletions and array replacement.", "medium", MEDIUM_WEIGHT, "deepMerge",
        "Implement deepMerge(input), where input is {base,overlay}. Plain objects merge recursively. Arrays and primitive values replace the base value. An overlay value equal to the string '__DELETE__' removes that key. A missing overlay key preserves the base key. Return a new value and never mutate either input.",
        r#"function deepMerge(input){function obj(x){return x&&typeof x==='object'&&!Array.isArray(x)}function go(a,b){if(b==='__DELETE__')return undefined;if(!obj(a)||!obj(b))return Array.isArray(b)?b.map(x=>obj(x)?go({},x):x):b;const o={};for(const k of Object.keys(a))o[k]=Array.isArray(a[k])?a[k].slice():obj(a[k])?go({},a[k]):a[k];for(const k of Object.keys(b)){const v=go(a[k],b[k]);if(v===undefined)delete o[k];else o[k]=v}return o}return go(input.base,input.overlay)}"#,
        vec![
            test("m2-1","nested",json!({"base":{"db":{"host":"a","port":1},"x":1},"overlay":{"db":{"port":2}}}),json!({"db":{"host":"a","port":2},"x":1})),
            test("m2-2","array replace",json!({"base":{"a":[1,2]},"overlay":{"a":[3]}}),json!({"a":[3]})),
            test("m2-3","delete",json!({"base":{"a":1,"b":2},"overlay":{"a":"__DELETE__"}}),json!({"b":2})),
            test("m2-4","object replaces primitive",json!({"base":{"a":1},"overlay":{"a":{"b":2}}}),json!({"a":{"b":2}})),
            test("m2-5","null replace",json!({"base":{"a":{"b":1}},"overlay":{"a":null}}),json!({"a":null})),
            test("m2-6","top-level array",json!({"base":{"a":1},"overlay":[1,{"x":2}]}),json!([1,{"x":2}])),
        ])
}

fn expression_case() -> BenchmarkCase {
    case("professional-js-v2-medium-03-expression", "Evaluate an arithmetic expression", "Tokenize and evaluate precedence, parentheses, variables, and unary signs.", "medium", MEDIUM_WEIGHT, "evaluateExpression",
        "Implement evaluateExpression(input), where input is {expression,variables}. Support decimal numbers, variable names matching [A-Za-z_][A-Za-z0-9_]*, whitespace, parentheses, binary + - * /, and unary +/-. Use normal precedence and left associativity. Return the numeric result. Throw for unknown variables, malformed expressions, or division by zero.",
        r#"function evaluateExpression(input){const s=input.expression;let i=0;function ws(){while(/\s/.test(s[i]||''))i++}function primary(){ws();if(s[i]==='('){i++;const v=add();ws();if(s[i++]!==')')throw Error();return v}const m=s.slice(i).match(/^(?:\d+(?:\.\d*)?|\.\d+|[A-Za-z_][A-Za-z0-9_]*)/);if(!m)throw Error();i+=m[0].length;if(/^[A-Za-z_]/.test(m[0])){if(!Object.prototype.hasOwnProperty.call(input.variables,m[0]))throw Error();return input.variables[m[0]]}return Number(m[0])}function unary(){ws();if(s[i]==='+'){i++;return unary()}if(s[i]==='-'){i++;return-unary()}return primary()}function mul(){let v=unary();for(;;){ws();const op=s[i];if(op!=='*'&&op!=='/')break;i++;const r=unary();if(op==='/'&&r===0)throw Error();v=op==='*'?v*r:v/r}return v}function add(){let v=mul();for(;;){ws();const op=s[i];if(op!=='+'&&op!=='-')break;i++;const r=mul();v=op==='+'?v+r:v-r}return v}const v=add();ws();if(i!==s.length)throw Error();return v}"#,
        vec![
            test("m3-1","precedence",json!({"expression":"2 + 3 * 4","variables":{}}),json!(14)),
            test("m3-2","parentheses",json!({"expression":"(2 + 3) * 4","variables":{}}),json!(20)),
            test("m3-3","unary",json!({"expression":"-x * -(2 + y)","variables":{"x":3,"y":1}}),json!(9)),
            test("m3-4","division",json!({"expression":"10 / 4 + .5","variables":{}}),json!(3)),
            test("m3-5","left associative",json!({"expression":"20 / 5 / 2","variables":{}}),json!(2)),
            test("m3-6","identifier",json!({"expression":"rate_2 * 3 - 1","variables":{"rate_2":2.5}}),json!(6.5)),
        ])
}

fn sessionize_case() -> BenchmarkCase {
    case("professional-js-v2-medium-04-sessionize", "Sessionize an event stream", "Sort out-of-order events and split deterministic user sessions.", "medium", MEDIUM_WEIGHT, "sessionizeEvents",
        "Implement sessionizeEvents(input), where input is {events,gap}. Each event has id,user,time. Deduplicate by id, keeping the first occurrence in the original array. Group by user. Sort accepted events by time then id. Start a new session when the difference from the previous event is strictly greater than gap. Return sessions sorted by user then start then first event id as {user,start,end,eventIds}. Do not mutate input.",
        r#"function sessionizeEvents(input){const seen=new Set(),users=new Map();for(const e of input.events){if(seen.has(e.id))continue;seen.add(e.id);if(!users.has(e.user))users.set(e.user,[]);users.get(e.user).push(e)}const out=[];for(const [user,es] of users){es.sort((a,b)=>a.time-b.time||a.id.localeCompare(b.id));let s;for(const e of es){if(!s||e.time-s.end>input.gap){s={user,start:e.time,end:e.time,eventIds:[e.id]};out.push(s)}else{s.end=e.time;s.eventIds.push(e.id)}}}return out.sort((a,b)=>a.user.localeCompare(b.user)||a.start-b.start||a.eventIds[0].localeCompare(b.eventIds[0]))}"#,
        vec![
            test("m4-1","out of order",json!({"gap":10,"events":[{"id":"c","user":"u","time":30},{"id":"a","user":"u","time":0},{"id":"b","user":"u","time":10}]}),json!([{"user":"u","start":0,"end":10,"eventIds":["a","b"]},{"user":"u","start":30,"end":30,"eventIds":["c"]}])),
            test("m4-2","gap boundary",json!({"gap":5,"events":[{"id":"a","user":"u","time":1},{"id":"b","user":"u","time":6}]}),json!([{"user":"u","start":1,"end":6,"eventIds":["a","b"]}])),
            test("m4-3","users",json!({"gap":1,"events":[{"id":"z","user":"b","time":0},{"id":"a","user":"a","time":2}]}),json!([{"user":"a","start":2,"end":2,"eventIds":["a"]},{"user":"b","start":0,"end":0,"eventIds":["z"]}])),
            test("m4-4","duplicate id first wins",json!({"gap":10,"events":[{"id":"x","user":"a","time":9},{"id":"x","user":"b","time":0}]}),json!([{"user":"a","start":9,"end":9,"eventIds":["x"]}])),
            test("m4-5","same time id order",json!({"gap":0,"events":[{"id":"b","user":"u","time":1},{"id":"a","user":"u","time":1}]}),json!([{"user":"u","start":1,"end":1,"eventIds":["a","b"]}])),
            test("m4-6","empty",json!({"gap":3,"events":[]}),json!([])),
        ])
}

fn json_patch_case() -> BenchmarkCase {
    case("professional-js-v2-medium-05-json-patch", "Apply JSON Patch operations", "Interpret escaped JSON pointers across objects and arrays.", "medium", MEDIUM_WEIGHT, "applyJsonPatch",
        "Implement applyJsonPatch(input), where input is {document,operations}. Return a deep-cloned document after applying operations in order. Support add, remove, and replace using RFC 6901 pointer decoding (~1 means / and ~0 means ~). For arrays, add with '-' appends, numeric add inserts, remove deletes, and replace overwrites. The empty path targets the whole document. Throw for an invalid path or index. Do not mutate input.",
        r#"function applyJsonPatch(input){let doc=JSON.parse(JSON.stringify(input.document));const parts=p=>p===''?[]:p.split('/').slice(1).map(x=>x.replace(/~1/g,'/').replace(/~0/g,'~'));for(const op of input.operations){const ps=parts(op.path);if(!ps.length){if(op.op==='remove')doc=null;else doc=JSON.parse(JSON.stringify(op.value));continue}let p=doc;for(let i=0;i<ps.length-1;i++){if(p==null||!(ps[i] in p))throw Error();p=p[ps[i]]}const k=ps.at(-1);if(Array.isArray(p)){if(op.op==='add'){const n=k==='-'?p.length:Number(k);if(!Number.isInteger(n)||n<0||n>p.length)throw Error();p.splice(n,0,JSON.parse(JSON.stringify(op.value)))}else{const n=Number(k);if(!Number.isInteger(n)||n<0||n>=p.length)throw Error();if(op.op==='remove')p.splice(n,1);else p[n]=JSON.parse(JSON.stringify(op.value))}}else{if(p==null||typeof p!=='object')throw Error();if(op.op!=='add'&&!Object.prototype.hasOwnProperty.call(p,k))throw Error();if(op.op==='remove')delete p[k];else p[k]=JSON.parse(JSON.stringify(op.value))}}return doc}"#,
        vec![
            test("m5-1","object operations",json!({"document":{"a":1,"b":2},"operations":[{"op":"replace","path":"/a","value":3},{"op":"remove","path":"/b"},{"op":"add","path":"/c","value":4}]}),json!({"a":3,"c":4})),
            test("m5-2","array insert append",json!({"document":{"a":[1,3]},"operations":[{"op":"add","path":"/a/1","value":2},{"op":"add","path":"/a/-","value":4}]}),json!({"a":[1,2,3,4]})),
            test("m5-3","array remove",json!({"document":["a","b","c"],"operations":[{"op":"remove","path":"/1"}]}),json!(["a","c"])),
            test("m5-4","escaped pointer",json!({"document":{"a/b":{"~x":1}},"operations":[{"op":"replace","path":"/a~1b/~0x","value":2}]}),json!({"a/b":{"~x":2}})),
            test("m5-5","root replace",json!({"document":{"a":1},"operations":[{"op":"replace","path":"","value":[1,2]}]}),json!([1,2])),
            test("m5-6","sequential",json!({"document":{},"operations":[{"op":"add","path":"/a","value":[]},{"op":"add","path":"/a/-","value":{"x":1}}]}),json!({"a":[{"x":1}]})),
        ])
}

fn allocate_case() -> BenchmarkCase {
    case("professional-js-v2-medium-06-capacity-allocation", "Allocate constrained capacity", "Apply multi-key ordering and deterministic best-fit allocation.", "medium", MEDIUM_WEIGHT, "allocateCapacity",
        "Implement allocateCapacity(input), where input is {capacities,requests}. capacities maps pool names to nonnegative integers. Each request has id, priority, amount, and pools. Process requests by descending priority, then ascending id. For each request, consider allowed pools with enough remaining capacity; choose the pool that leaves the smallest remainder, breaking ties lexically. Allocate the whole request or reject it. Return {allocations,rejected,remaining}, where each allocation is exactly {id,pool}, allocations are in processing order, rejected ids are in processing order, and remaining object keys are sorted lexically. Do not mutate input.",
        r#"function allocateCapacity(input){const rem={...input.capacities},allocations=[],rejected=[];const rs=input.requests.slice().sort((a,b)=>b.priority-a.priority||a.id.localeCompare(b.id));for(const r of rs){const p=r.pools.filter(x=>(rem[x]??-1)>=r.amount).sort((a,b)=>(rem[a]-r.amount)-(rem[b]-r.amount)||a.localeCompare(b))[0];if(p==null)rejected.push(r.id);else{rem[p]-=r.amount;allocations.push({id:r.id,pool:p})}}const remaining={};for(const k of Object.keys(rem).sort())remaining[k]=rem[k];return{allocations,rejected,remaining}}"#,
        vec![
            test("m6-1","priority best fit",json!({"capacities":{"a":5,"b":8},"requests":[{"id":"low","priority":1,"amount":5,"pools":["a","b"]},{"id":"high","priority":2,"amount":4,"pools":["a","b"]}]}),json!({"allocations":[{"id":"high","pool":"a"},{"id":"low","pool":"b"}],"rejected":[],"remaining":{"a":1,"b":3}})),
            test("m6-2","lexical ties",json!({"capacities":{"z":5,"a":5},"requests":[{"id":"r","priority":1,"amount":2,"pools":["z","a"]}]}),json!({"allocations":[{"id":"r","pool":"a"}],"rejected":[],"remaining":{"a":3,"z":5}})),
            test("m6-3","request id order",json!({"capacities":{"p":3},"requests":[{"id":"b","priority":1,"amount":2,"pools":["p"]},{"id":"a","priority":1,"amount":2,"pools":["p"]}]}),json!({"allocations":[{"id":"a","pool":"p"}],"rejected":["b"],"remaining":{"p":1}})),
            test("m6-4","unknown pool",json!({"capacities":{"p":1},"requests":[{"id":"x","priority":1,"amount":1,"pools":["q"]}]}),json!({"allocations":[],"rejected":["x"],"remaining":{"p":1}})),
            test("m6-5","zero request",json!({"capacities":{"b":0,"a":0},"requests":[{"id":"x","priority":0,"amount":0,"pools":["b","a"]}]}),json!({"allocations":[{"id":"x","pool":"a"}],"rejected":[],"remaining":{"a":0,"b":0}})),
            test("m6-6","empty",json!({"capacities":{},"requests":[]}),json!({"allocations":[],"rejected":[],"remaining":{}})),
        ])
}

fn semver_case() -> BenchmarkCase {
    case("professional-js-v2-hard-02-semver-select", "Resolve a semantic-version range", "Parse compound ranges and choose the greatest stable matching version.", "hard", HARD_WEIGHT, "selectVersion",
        "Implement selectVersion(input), where input is {versions,range}. Versions are valid major.minor.patch strings with an optional '-prerelease' suffix. Ignore prerelease versions unless the entire range text contains '-'. A range is one or more OR branches separated by '||'; each branch is whitespace-separated AND comparators. Support exact versions, > >= < <=, caret (^), tilde (~), and wildcards x, X, or * in minor/patch positions. Missing minor or patch acts as a wildcard. Return the greatest matching original version by numeric major/minor/patch, with stable greater than prerelease at the same numbers; compare prerelease dot identifiers using SemVer numeric-vs-text rules. Return null when none match.",
        r#"function selectVersion(input){function pv(s){const [core,pre]=s.split('-',2),a=core.split('.').map(Number);return{raw:s,a:[a[0]||0,a[1]||0,a[2]||0],pre:pre==null?null:pre.split('.')}}function cmp(a,b){for(let i=0;i<3;i++)if(a.a[i]!==b.a[i])return a.a[i]-b.a[i];if(a.pre==null||b.pre==null)return a.pre==null?(b.pre==null?0:1):-1;for(let i=0;i<Math.max(a.pre.length,b.pre.length);i++){if(a.pre[i]==null)return-1;if(b.pre[i]==null)return 1;const x=a.pre[i],y=b.pre[i],xn=/^\d+$/.test(x),yn=/^\d+$/.test(y);if(x===y)continue;if(xn&&yn)return Number(x)-Number(y);if(xn!==yn)return xn?-1:1;return x<y?-1:1}return 0}function one(v,t){let op='';const m=t.match(/^(\^|~|>=|<=|>|<)?(.*)$/);op=m[1]||'';let q=m[2];if(/^(x|\*)$/i.test(q))return true;let parts=q.split('.'),wild=parts.findIndex(x=>/^(x|\*)$/i.test(x));if(wild<0&&parts.length<3&&!q.includes('-'))wild=parts.length;const nums=parts.map(x=>Number((x||'0').split('-')[0])||0);const low=pv([nums[0]||0,nums[1]||0,nums[2]||0].join('.')+(q.includes('-')?'-'+q.split('-')[1]:''));if(wild>=0){if(cmp(v,low)<0)return false;const hi=pv(wild<=1?(low.a[0]+1)+'.0.0':low.a[0]+'.'+(low.a[1]+1)+'.0');return cmp(v,hi)<0}if(op==='^'){const hi=pv(low.a[0]>0?(low.a[0]+1)+'.0.0':low.a[1]>0?'0.'+(low.a[1]+1)+'.0':'0.0.'+(low.a[2]+1));return cmp(v,low)>=0&&cmp(v,hi)<0}if(op==='~'){const hi=pv(low.a[0]+'.'+(low.a[1]+1)+'.0');return cmp(v,low)>=0&&cmp(v,hi)<0}const c=cmp(v,low);return op==='>'?c>0:op==='>='?c>=0:op==='<'?c<0:op==='<='?c<=0:c===0}const allowPre=input.range.includes('-'),branches=input.range.split('||').map(x=>x.trim().split(/\s+/).filter(Boolean));return input.versions.map(pv).filter(v=>(allowPre||v.pre==null)&&branches.some(b=>b.every(t=>one(v,t)))).sort(cmp).at(-1)?.raw??null}"#,
        vec![
            test("h2-1","caret",json!({"versions":["1.2.0","1.9.9","2.0.0"],"range":"^1.2.0"}),json!("1.9.9")),
            test("h2-2","zero caret",json!({"versions":["0.2.3","0.2.9","0.3.0"],"range":"^0.2.3"}),json!("0.2.9")),
            test("h2-3","and comparators",json!({"versions":["1.4.9","1.5.0","1.9.0","2.0.0"],"range":">=1.5.0 <2.0.0"}),json!("1.9.0")),
            test("h2-4","or and wildcard",json!({"versions":["1.9.0","2.1.0","2.4.0","3.0.0"],"range":"1.x || 2.1.x"}),json!("2.1.0")),
            test("h2-5","prerelease excluded",json!({"versions":["2.0.0-beta.2","2.0.0-beta.10","1.9.0"],"range":"*"}),json!("1.9.0")),
            test("h2-6","prerelease included",json!({"versions":["2.0.0-beta.2","2.0.0-beta.10","1.9.0"],"range":">=2.0.0-beta.1 <2.0.0"}),json!("2.0.0-beta.10")),
        ])
}

fn sheet_case() -> BenchmarkCase {
    case("professional-js-v2-hard-03-sheet-evaluator", "Evaluate a dependency spreadsheet", "Evaluate nested expression trees with references, cycles, and propagated errors.", "hard", HARD_WEIGHT, "evaluateSheet",
        "Implement evaluateSheet(cells). cells maps names to either a finite number or an expression object {op,args}. Each arg is a number, a cell-name string reference, or another expression. Supported ops are add, sub, mul, div, sum, min, max. add/mul/sum accept any number of args; sub and div require exactly two; min/max require at least one. Evaluate references lazily. Missing references produce 'missing:<name>', dependency cycles produce 'cycle', division by zero produces 'division-by-zero', and other invalid expressions produce 'invalid'. An error propagates to dependents unchanged. Return {values,errors}; both objects must have cell keys inserted in lexical order, and only successful cells appear in values. Do not mutate input.",
        r#"function evaluateSheet(cells){const state=new Map(),memo=new Map();function calc(x){if(typeof x==='number')return{v:x};if(typeof x==='string')return cell(x);if(!x||typeof x!=='object'||!Array.isArray(x.args))return{e:'invalid'};const a=x.args.map(calc),bad=a.find(y=>y.e);if(bad)return bad;const v=a.map(y=>y.v);if((x.op==='sub'||x.op==='div')&&v.length!==2||(['min','max'].includes(x.op)&&!v.length))return{e:'invalid'};if(x.op==='div'&&v[1]===0)return{e:'division-by-zero'};const ops={add:()=>v.reduce((p,q)=>p+q,0),sum:()=>v.reduce((p,q)=>p+q,0),mul:()=>v.reduce((p,q)=>p*q,1),sub:()=>v[0]-v[1],div:()=>v[0]/v[1],min:()=>Math.min(...v),max:()=>Math.max(...v)};return ops[x.op]?{v:ops[x.op]()}:{e:'invalid'}}function cell(n){if(!(n in cells))return{e:'missing:'+n};if(state.get(n)===1)return{e:'cycle'};if(state.get(n)===2)return memo.get(n);state.set(n,1);const r=calc(cells[n]);state.set(n,2);memo.set(n,r);return r}const values={},errors={};for(const n of Object.keys(cells).sort()){const r=cell(n);if(r.e)errors[n]=r.e;else values[n]=r.v}return{values,errors}}"#,
        vec![
            test("h3-1","dependency graph",json!({"A1":2,"A2":{"op":"mul","args":["A1",3]},"B1":{"op":"add","args":["A2",4]}}),json!({"values":{"A1":2,"A2":6,"B1":10},"errors":{}})),
            test("h3-2","nested functions",json!({"A":{"op":"sum","args":[1,{"op":"max","args":[2,5]},3]}}),json!({"values":{"A":9},"errors":{}})),
            test("h3-3","missing propagation",json!({"A":{"op":"add","args":["NOPE",1]},"B":{"op":"mul","args":["A",2]}}),json!({"values":{},"errors":{"A":"missing:NOPE","B":"missing:NOPE"}})),
            test("h3-4","cycle propagation",json!({"A":{"op":"add","args":["B",1]},"B":{"op":"add","args":["A",1]},"C":{"op":"add","args":["A",1]}}),json!({"values":{},"errors":{"A":"cycle","B":"cycle","C":"cycle"}})),
            test("h3-5","division error",json!({"A":{"op":"div","args":[10,0]},"B":3}),json!({"values":{"B":3},"errors":{"A":"division-by-zero"}})),
            test("h3-6","invalid arity",json!({"A":{"op":"sub","args":[1]},"Z":0}),json!({"values":{"Z":0},"errors":{"A":"invalid"}})),
        ])
}

fn route_case() -> BenchmarkCase {
    case("professional-js-v2-hard-04-constrained-route", "Find a constrained route", "Optimize a directed route under cost, time, stop, and exclusion constraints.", "hard", HARD_WEIGHT, "findRoute",
        "Implement findRoute(input). input has directed edges {from,to,cost,minutes}, start, end, maxEdges, and banned node names. A valid route uses at most maxEdges edges and may not visit banned nodes (including endpoints). Minimize total cost, then total minutes, then edge count, then the entire node path lexically (compare path.join('\\0')). Nodes may not repeat. Return {path,cost,minutes} or null. Do not mutate input.",
        r#"function findRoute(input){if(input.banned.includes(input.start)||input.banned.includes(input.end))return null;const banned=new Set(input.banned);let best=null;function better(x,y){if(!y)return true;return x.cost!==y.cost?x.cost<y.cost:x.minutes!==y.minutes?x.minutes<y.minutes:x.path.length!==y.path.length?x.path.length<y.path.length:x.path.join('\0')<y.path.join('\0')}function dfs(node,path,cost,minutes){if(path.length-1>input.maxEdges)return;if(node===input.end){const r={path:path.slice(),cost,minutes};if(better(r,best))best=r;return}for(const e of input.edges){if(e.from!==node||banned.has(e.to)||path.includes(e.to))continue;dfs(e.to,[...path,e.to],cost+e.cost,minutes+e.minutes)}}dfs(input.start,[input.start],0,0);return best}"#,
        vec![
            test("h4-1","cost before time",json!({"start":"A","end":"D","maxEdges":3,"banned":[],"edges":[{"from":"A","to":"D","cost":5,"minutes":1},{"from":"A","to":"B","cost":2,"minutes":5},{"from":"B","to":"D","cost":2,"minutes":5}]}),json!({"path":["A","B","D"],"cost":4,"minutes":10})),
            test("h4-2","time tie break",json!({"start":"A","end":"D","maxEdges":3,"banned":[],"edges":[{"from":"A","to":"B","cost":1,"minutes":5},{"from":"B","to":"D","cost":1,"minutes":5},{"from":"A","to":"C","cost":1,"minutes":2},{"from":"C","to":"D","cost":1,"minutes":2}]}),json!({"path":["A","C","D"],"cost":2,"minutes":4})),
            test("h4-3","lexical path tie",json!({"start":"A","end":"D","maxEdges":2,"banned":[],"edges":[{"from":"A","to":"C","cost":1,"minutes":1},{"from":"C","to":"D","cost":1,"minutes":1},{"from":"A","to":"B","cost":1,"minutes":1},{"from":"B","to":"D","cost":1,"minutes":1}]}),json!({"path":["A","B","D"],"cost":2,"minutes":2})),
            test("h4-4","edge limit",json!({"start":"A","end":"D","maxEdges":1,"banned":[],"edges":[{"from":"A","to":"B","cost":1,"minutes":1},{"from":"B","to":"D","cost":1,"minutes":1}]}),Value::Null),
            test("h4-5","banned",json!({"start":"A","end":"D","maxEdges":3,"banned":["B"],"edges":[{"from":"A","to":"B","cost":1,"minutes":1},{"from":"B","to":"D","cost":1,"minutes":1},{"from":"A","to":"D","cost":9,"minutes":9}]}),json!({"path":["A","D"],"cost":9,"minutes":9})),
            test("h4-6","avoid cycles",json!({"start":"A","end":"C","maxEdges":4,"banned":[],"edges":[{"from":"A","to":"B","cost":1,"minutes":1},{"from":"B","to":"A","cost":-5,"minutes":1},{"from":"B","to":"C","cost":2,"minutes":2}]}),json!({"path":["A","B","C"],"cost":3,"minutes":3})),
        ])
}

fn schedule_case() -> BenchmarkCase {
    case("professional-js-v2-hard-05-job-scheduler", "Simulate a constrained job scheduler", "Combine dependencies, worker eligibility, priorities, and deterministic event simulation.", "hard", HARD_WEIGHT, "scheduleJobs",
        "Implement scheduleJobs(input), where input is {workers,jobs}. workers is an array of unique names. Each job has id,duration,priority,deps,workers (eligible names). First mark as blocked every job with a missing dependency or no existing eligible worker, plus every job depending directly or indirectly on blocked jobs or a dependency cycle. Simulate the rest from time 0. At each time, finish all jobs ending then; repeatedly choose the ready unscheduled job by priority descending, duration ascending, id ascending, and assign it to its lexically first idle eligible worker until no assignment is possible. Advance to the next finish time. Return {assignments,blocked,makespan}; assignments are ordered by start, worker, job and contain {job,worker,start,end}; blocked is lexical. Do not mutate input.",
        r#"function scheduleJobs(input){const wm=new Set(input.workers),m=new Map(input.jobs.map(j=>[j.id,{...j,deps:[...new Set(j.deps)]}])),blocked=new Set();for(const j of m.values())if(j.deps.some(d=>!m.has(d))||!j.workers.some(w=>wm.has(w)))blocked.add(j.id);let change=true;while(change){change=false;for(const j of m.values())if(!blocked.has(j.id)&&j.deps.some(d=>blocked.has(d))){blocked.add(j.id);change=true}}const valid=[...m.keys()].filter(x=>!blocked.has(x)),done=new Set(),running=[],scheduled=new Set(),out=[];let time=0;while(true){for(let i=running.length-1;i>=0;i--)if(running[i].end===time){done.add(running[i].job);running.splice(i,1)}const idle=input.workers.filter(w=>!running.some(r=>r.worker===w)).sort();let progress=true;while(progress){progress=false;const ready=valid.filter(id=>!scheduled.has(id)&&m.get(id).deps.every(d=>done.has(d))).sort((a,b)=>m.get(b).priority-m.get(a).priority||m.get(a).duration-m.get(b).duration||a.localeCompare(b));for(const id of ready){const j=m.get(id),wi=idle.findIndex(w=>j.workers.includes(w));if(wi>=0){const worker=idle.splice(wi,1)[0],r={job:id,worker,start:time,end:time+j.duration};running.push(r);out.push(r);scheduled.add(id);progress=true;break}}}if(!running.length){for(const id of valid)if(!scheduled.has(id))blocked.add(id);break}time=Math.min(...running.map(r=>r.end))}out.sort((a,b)=>a.start-b.start||a.worker.localeCompare(b.worker)||a.job.localeCompare(b.job));return{assignments:out,blocked:[...blocked].sort(),makespan:out.length?Math.max(...out.map(x=>x.end)):0}}"#,
        vec![
            test("h5-1","parallel dependencies",json!({"workers":["w2","w1"],"jobs":[{"id":"a","duration":3,"priority":1,"deps":[],"workers":["w1"]},{"id":"b","duration":2,"priority":1,"deps":[],"workers":["w2"]},{"id":"c","duration":1,"priority":1,"deps":["a","b"],"workers":["w1","w2"]}]}),json!({"assignments":[{"job":"a","worker":"w1","start":0,"end":3},{"job":"b","worker":"w2","start":0,"end":2},{"job":"c","worker":"w1","start":3,"end":4}],"blocked":[],"makespan":4})),
            test("h5-2","priority and duration",json!({"workers":["w"],"jobs":[{"id":"low","duration":1,"priority":1,"deps":[],"workers":["w"]},{"id":"long","duration":5,"priority":2,"deps":[],"workers":["w"]},{"id":"short","duration":2,"priority":2,"deps":[],"workers":["w"]}]}),json!({"assignments":[{"job":"short","worker":"w","start":0,"end":2},{"job":"long","worker":"w","start":2,"end":7},{"job":"low","worker":"w","start":7,"end":8}],"blocked":[],"makespan":8})),
            test("h5-3","missing propagation",json!({"workers":["w"],"jobs":[{"id":"a","duration":1,"priority":1,"deps":["x"],"workers":["w"]},{"id":"b","duration":1,"priority":1,"deps":["a"],"workers":["w"]}]}),json!({"assignments":[],"blocked":["a","b"],"makespan":0})),
            test("h5-4","cycle and valid",json!({"workers":["w"],"jobs":[{"id":"a","duration":1,"priority":1,"deps":["b"],"workers":["w"]},{"id":"b","duration":1,"priority":1,"deps":["a"],"workers":["w"]},{"id":"ok","duration":2,"priority":1,"deps":[],"workers":["w"]}]}),json!({"assignments":[{"job":"ok","worker":"w","start":0,"end":2}],"blocked":["a","b"],"makespan":2})),
            test("h5-5","worker lexical",json!({"workers":["z","a"],"jobs":[{"id":"x","duration":1,"priority":1,"deps":[],"workers":["z","a"]}]}),json!({"assignments":[{"job":"x","worker":"a","start":0,"end":1}],"blocked":[],"makespan":1})),
            test("h5-6","no eligible worker",json!({"workers":["a"],"jobs":[{"id":"x","duration":1,"priority":1,"deps":[],"workers":["b"]}]}),json!({"assignments":[],"blocked":["x"],"makespan":0})),
        ])
}

fn three_way_merge_case() -> BenchmarkCase {
    case("professional-js-v2-hard-06-three-way-merge", "Perform a recursive three-way merge", "Merge JSON trees with deletions and precise conflict pointers.", "hard", HARD_WEIGHT, "threeWayMerge",
        "Implement threeWayMerge(input), where input is {base,left,right}. Recursively merge JSON values. If left and right are deeply equal, use that value. If left equals base, use right; if right equals base, use left. If all three current values are plain objects, recursively merge the union of keys. A missing object key represents deletion and differs from a present null. Otherwise record a conflict at that RFC 6901 JSON pointer and choose the left value; if left is missing, omit it. Return {value,conflicts}, with conflict pointers sorted lexically. Escape '~' as '~0' and '/' as '~1'. Inputs must not be mutated.",
        r#"function threeWayMerge(input){const MISS=Symbol(),conflicts=[];const eq=(a,b)=>a===MISS||b===MISS?a===b:JSON.stringify(a)===JSON.stringify(b),obj=x=>x!==MISS&&x!==null&&typeof x==='object'&&!Array.isArray(x),esc=s=>s.replace(/~/g,'~0').replace(/\//g,'~1');function go(b,l,r,p){if(eq(l,r))return l;if(eq(l,b))return r;if(eq(r,b))return l;if(obj(b)&&obj(l)&&obj(r)){const o={};for(const k of [...new Set([...Object.keys(b),...Object.keys(l),...Object.keys(r)])].sort()){const v=go(k in b?b[k]:MISS,k in l?l[k]:MISS,k in r?r[k]:MISS,p+'/'+esc(k));if(v!==MISS)o[k]=v}return o}conflicts.push(p||'');return l}const value=go(input.base,input.left,input.right,'');return{value:value===MISS?null:value,conflicts:conflicts.sort()}}"#,
        vec![
            test("h6-1","independent edits",json!({"base":{"a":1,"b":1},"left":{"a":2,"b":1},"right":{"a":1,"b":3}}),json!({"value":{"a":2,"b":3},"conflicts":[]})),
            test("h6-2","same edit",json!({"base":{"a":1},"left":{"a":2},"right":{"a":2}}),json!({"value":{"a":2},"conflicts":[]})),
            test("h6-3","scalar conflict",json!({"base":{"a":1},"left":{"a":2},"right":{"a":3}}),json!({"value":{"a":2},"conflicts":["/a"]})),
            test("h6-4","delete versus edit",json!({"base":{"a":{"x":1}},"left":{},"right":{"a":{"x":2}}}),json!({"value":{},"conflicts":["/a"]})),
            test("h6-5","nested and escaped",json!({"base":{"a/b":{"~x":1}},"left":{"a/b":{"~x":2}},"right":{"a/b":{"~x":3}}}),json!({"value":{"a/b":{"~x":2}},"conflicts":["/a~1b/~0x"]})),
            test("h6-6","array conflict",json!({"base":{"a":[1]},"left":{"a":[1,2]},"right":{"a":[1,3]}}),json!({"value":{"a":[1,2]},"conflicts":["/a"]})),
        ])
}
