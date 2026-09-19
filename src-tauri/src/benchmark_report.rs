//! Self-contained offline benchmark report generation.

use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkReportRecord {
    pub profile_id: String,
    pub alias: String,
    pub benchmark_id: String,
    pub benchmark_title: String,
    pub benchmark_kind: String,
    pub difficulty: Option<String>,
    pub weight: Option<f64>,
    pub attempt: u32,
    pub status: String,
    pub score: Option<f64>,
    pub passed: Option<usize>,
    pub total: Option<usize>,
    pub duration_seconds: Option<f64>,
    pub tokens_per_second: Option<f64>,
    pub draft_tokens: Option<u64>,
    pub accepted_draft_tokens: Option<u64>,
    pub speculative_acceptance_rate: Option<f64>,
    pub output_path: String,
    pub feedback: Vec<String>,
}

pub fn write_html_report(
    output_dir: &Path,
    records: &[BenchmarkReportRecord],
) -> Result<PathBuf, String> {
    let report_path = output_dir.join("benchmark-report.html");
    let summary_path = output_dir.join("summary.json");
    let data = serde_json::to_string(records).map_err(|error| error.to_string())?;
    let safe_data = data
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace("</script", "\\u003c/script");
    let summary = json!({
        "recordCount": records.len(),
        "professionalRecords": records.iter().filter(|record| record.benchmark_kind == "professional").count(),
        "customRecords": records.iter().filter(|record| record.benchmark_kind == "custom").count(),
        "records": records,
        "generatedAt": chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    std::fs::create_dir_all(output_dir).map_err(|error| error.to_string())?;
    std::fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let html = REPORT_TEMPLATE.replace("__REPORT_DATA__", &safe_data);
    let temp_path = output_dir.join("benchmark-report.html.tmp");
    std::fs::write(&temp_path, html).map_err(|error| error.to_string())?;
    if report_path.exists() {
        std::fs::remove_file(&report_path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temp_path, &report_path).map_err(|error| error.to_string())?;
    Ok(report_path)
}

const REPORT_TEMPLATE: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Llama Switcher benchmark report</title>
<style>
:root{color-scheme:dark;--bg:#11151d;--panel:#1a202b;--panel2:#202938;--text:#ecf1f8;--muted:#9aa8ba;--grid:#3a4555;--accent:#75aaf8;--good:#52d273;--caution:#f0c674;--warning:#f39c4a;--bad:#ee8181}
*{box-sizing:border-box}body{margin:0;background:linear-gradient(150deg,#11151d,#171d27);color:var(--text);font:14px/1.45 system-ui,-apple-system,Segoe UI,sans-serif}main{max-width:1500px;margin:auto;padding:36px 34px 70px}h1{font-size:30px;margin:0 0 5px}h2{font-size:18px;margin:0 0 14px}p{color:var(--muted);margin:0}.header{display:flex;justify-content:space-between;gap:24px;align-items:flex-end;margin-bottom:26px}.header small{display:block;color:var(--muted);text-align:right}.cards{display:grid;grid-template-columns:repeat(5,minmax(0,1fr));gap:10px;margin-bottom:24px}.card,.panel{border:1px solid #2b3645;border-radius:14px;background:linear-gradient(180deg,rgba(255,255,255,.035),transparent),var(--panel);box-shadow:0 12px 28px rgba(0,0,0,.2)}.card{padding:14px}.card b{display:block;font-size:22px;font-variant-numeric:tabular-nums}.card span{display:block;color:var(--muted);font-size:11px;text-transform:uppercase;letter-spacing:.08em}.panel{padding:20px;margin-bottom:18px}.chart-wrap{overflow:auto}.chart{width:100%;min-width:720px;height:auto}.legend{display:flex;flex-wrap:wrap;gap:9px 18px;margin:0 0 10px}.legend button{border:0;background:transparent;color:var(--text);cursor:pointer;padding:0;font-size:12px}.legend i{display:inline-block;width:11px;height:11px;border-radius:2px;margin-right:5px;vertical-align:-1px}.toolbar{display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin-bottom:14px}.toolbar button,.toolbar select{border:1px solid #3a475a;border-radius:7px;background:#151b25;color:var(--text);padding:7px 10px}.table-wrap{overflow:auto}table{width:100%;border-collapse:collapse;min-width:920px}th,td{text-align:left;border-bottom:1px solid #2d3745;padding:9px 8px;white-space:nowrap}th{font-size:10px;color:var(--muted);text-transform:uppercase;letter-spacing:.06em;cursor:pointer}td{font-variant-numeric:tabular-nums}.score{font-weight:700}.score-good{color:var(--good)}.score-caution{color:var(--caution)}.score-warning{color:var(--warning)}.score-bad,.failed{color:var(--bad)}details{border-top:1px solid #2d3745;padding:11px 0}details:first-of-type{border-top:0}summary{cursor:pointer;font-weight:650}.detail-grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:8px;margin-top:10px}.detail-grid div{background:var(--panel2);padding:8px;border-radius:7px}.detail-grid span{display:block;color:var(--muted);font-size:10px}.detail-grid b{display:block;margin-top:2px}.feedback{margin:9px 0 0;padding-left:20px;color:#ffc7c7}.empty{padding:25px;text-align:center;color:var(--muted)}.footer{color:var(--muted);font-size:11px;margin-top:22px}@media(max-width:900px){main{padding:24px 16px}.header{display:block}.header small{text-align:left;margin-top:10px}.cards{grid-template-columns:repeat(2,minmax(0,1fr))}.detail-grid{grid-template-columns:repeat(2,minmax(0,1fr))}}@media print{body{background:white;color:#111}main{max-width:none;padding:10px}.panel,.card{box-shadow:none;border-color:#bbb;background:white}.toolbar,.footer{display:none}.chart{min-width:0}}
</style>
</head>
<body>
<main>
<div class="header"><div><h1>Llama Switcher benchmark report</h1><p>Professional correctness and inference performance comparison</p></div><small id="generated"></small></div>
<section class="cards" id="cards"></section>
<section class="panel"><h2>Professional accuracy</h2><p>Grouped scores by difficulty, with correctness kept separate from generation speed.</p><div class="chart-wrap"><div id="accuracyLegend" class="legend"></div><svg id="accuracyChart" class="chart" viewBox="0 0 1200 500" role="img" aria-label="Professional accuracy chart"></svg></div></section>
<section class="panel"><h2>Completion speed</h2><p>Average completion tokens per second for every benchmark attempt.</p><div class="chart-wrap"><div id="speedLegend" class="legend"></div><svg id="speedChart" class="chart" viewBox="0 0 1200 430" role="img" aria-label="Completion speed chart"></svg></div></section>
<section class="panel"><h2>Correctness versus speed</h2><p>Models toward the upper-right combine stronger correctness with faster generation.</p><div class="chart-wrap"><svg id="scatterChart" class="chart" viewBox="0 0 1200 430" role="img" aria-label="Correctness versus speed chart"></svg></div></section>
<section class="panel"><h2>Model ranking</h2><div class="toolbar"><select id="difficulty"><option value="all">All professional cases</option><option value="easy">Easy</option><option value="medium">Medium</option><option value="hard">Hard</option></select><button id="toggleCustom">Show custom prompt performance</button></div><div class="table-wrap"><table id="ranking"><thead><tr><th data-key="overall">Overall</th><th data-key="alias">Model</th><th data-key="easy">Easy</th><th data-key="medium">Medium</th><th data-key="hard">Hard</th><th data-key="fullPass">Full pass</th><th data-key="speed">Tok/s</th><th data-key="duration">Time</th></tr></thead><tbody></tbody></table></div></section>
<section class="panel"><h2>Reliability</h2><p>Full-pass rate and score stability across repeated attempts.</p><div class="table-wrap"><table id="reliability"><thead><tr><th>Model</th><th>Attempts</th><th>Full pass</th><th>Mean score</th><th>Stability</th></tr></thead><tbody></tbody></table></div></section>
<section class="panel"><h2>Attempt details</h2><div id="details"></div></section>
<div class="footer">Scores are calculated from the bundled professional suite. Custom prompts are performance-only and are not included in correctness rankings.</div>
</main>
<script>
const DATA=__REPORT_DATA__;
const COLORS=['#f06b1a','#f3ae22','#d64bbd','#19c3c0','#20bd62','#d41473','#7e8af6','#e07bce','#a9c941','#ef5350'];
const esc=function(s){return String(s==null?'':s).replace(/[&<>"']/g,function(c){return {'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]})};
const professional=DATA.filter(function(r){return r.benchmarkKind==='professional'}),custom=DATA.filter(function(r){return r.benchmarkKind==='custom'});
const models=[...new Set(DATA.map(function(r){return r.alias}))],modelColor=new Map(models.map(function(m,i){return [m,COLORS[i%COLORS.length]]}));
let hidden=new Set(),sortKey='overall',customShown=false;
document.getElementById('generated').textContent=DATA.length+' attempts · '+new Date().toLocaleString();
function mean(xs){return xs.length?xs.reduce(function(a,b){return a+b},0)/xs.length:null}
function weightedScore(records){const xs=records.filter(function(r){return r.score!=null}),den=xs.reduce(function(a,r){return a+Number(r.weight||1)},0);return den?xs.reduce(function(a,r){return a+Number(r.score)*Number(r.weight||1)},0)/den:null}
function scoreClass(value){return 'score '+(value<40?'score-bad':value<60?'score-warning':value<90?'score-caution':'score-good')}
function grouped(records,field,categories){const by=new Map();models.forEach(function(model){const row=categories.map(function(category){const vals=records.filter(function(r){return r.alias===model&&(category==='overall'||r.difficulty===category)&&r[field]!=null}).map(function(r){return Number(r[field])});return mean(vals)});by.set(model,row)});return by}
function renderLegend(elId){const el=document.getElementById(elId);el.innerHTML='';models.forEach(function(model){const b=document.createElement('button');b.innerHTML='<i style="background:'+modelColor.get(model)+'"></i>'+esc(model);b.onclick=function(){hidden.has(model)?hidden.delete(model):hidden.add(model);renderAll()};el.appendChild(b)})}
function renderBars(svgId,legendId,records,field,categories,maxValue,axisLabel){renderLegend(legendId);const svg=document.getElementById(svgId),active=models.filter(function(m){return !hidden.has(m)}),w=1200,h=field==='score'?500:430,pad={l:72,r:25,t:25,b:86},cw=w-pad.l-pad.r,ch=h-pad.t-pad.b,data=grouped(records,field,categories);let out='';for(let i=0;i<=4;i++){const y=pad.t+ch-(i/4)*ch,v=maxValue*i/4;out+='<line x1="'+pad.l+'" x2="'+(w-pad.r)+'" y1="'+y+'" y2="'+y+'" stroke="#3a4555" stroke-dasharray="3 4"/><text x="'+(pad.l-10)+'" y="'+(y+4)+'" text-anchor="end" fill="#9aa8ba" font-size="12">'+(field==='score'?Math.round(v):v.toFixed(0))+'</text>'}const groupW=cw/categories.length,barW=Math.min(42,groupW/Math.max(active.length,1)*.72);categories.forEach(function(category,ci){const gx=pad.l+ci*groupW+groupW/2;out+='<text x="'+gx+'" y="'+(h-45)+'" text-anchor="middle" fill="#ecf1f8" font-size="13">'+esc(category)+'</text>';active.forEach(function(model,mi){const value=data.get(model)[ci];if(value==null)return;const x=gx-(active.length*barW)/2+mi*barW,bh=Math.max(0,(value/maxValue)*ch),y=pad.t+ch-bh;out+='<rect x="'+(x+1)+'" y="'+y+'" width="'+Math.max(4,barW-3)+'" height="'+bh+'" rx="4" fill="'+modelColor.get(model)+'"><title>'+esc(model)+' · '+esc(category)+': '+value.toFixed(1)+(field==='score'?'%':'')+'</title></rect><text x="'+(x+barW/2)+'" y="'+Math.max(15,y-6)+'" text-anchor="middle" fill="#ecf1f8" font-size="11">'+value.toFixed(1)+'</text>'})});out+='<text x="18" y="'+(pad.t+ch/2)+'" transform="rotate(-90 18 '+(pad.t+ch/2)+')" text-anchor="middle" fill="#9aa8ba" font-size="13">'+axisLabel+'</text>';svg.innerHTML=out}
function aggregates(filter){const rows=[];models.forEach(function(model){const all=professional.filter(function(r){return r.alias===model&&(filter==='all'||r.difficulty===filter)}),score=function(d){return mean(all.filter(function(r){return r.difficulty===d}).map(function(r){return r.score}).filter(function(v){return v!=null}))},attempts=new Map(),expected=filter==='all'?new Set(professional.filter(function(r){return r.alias===model}).map(function(r){return r.benchmarkId})).size:1;all.forEach(function(r){const k=String(r.attempt);if(!attempts.has(k))attempts.set(k,[]);attempts.get(k).push(r)});const full=[...attempts.values()].filter(function(a){return a.length>=expected&&a.every(function(r){return r.score===100})}).length;rows.push({alias:model,overall:weightedScore(all)||0,easy:score('easy')||0,medium:score('medium')||0,hard:score('hard')||0,fullPass:attempts.size?full/attempts.size*100:0,speed:mean(all.map(function(r){return r.tokensPerSecond}).filter(function(v){return v!=null}))||0,duration:mean(all.map(function(r){return r.durationSeconds}).filter(function(v){return v!=null}))||0})});return rows.sort(function(a,b){return b.overall-a.overall})}
function renderCards(){const rows=aggregates('all'),winner=rows[0],fastest=[...rows].filter(function(r){return r.speed}).sort(function(a,b){return b.speed-a.speed})[0],bestFull=[...rows].sort(function(a,b){return b.fullPass-a.fullPass})[0],modelRuns=new Set(professional.map(function(r){return r.alias+'::'+r.attempt})).size,cards=[['Winner',winner?winner.alias:'—'],['Top score',winner?winner.overall.toFixed(1)+'%':'—'],['Fastest',fastest?fastest.alias:'—'],['Best full pass',bestFull?bestFull.fullPass.toFixed(0)+'%':'—'],['Model runs',String(modelRuns)]];document.getElementById('cards').innerHTML=cards.map(function(x){return '<div class="card"><b>'+esc(x[1])+'</b><span>'+esc(x[0])+'</span></div>'}).join('')}
function renderRanking(){const filter=document.getElementById('difficulty').value,rows=aggregates(filter);rows.sort(function(a,b){const av=a[sortKey],bv=b[sortKey];return typeof av==='string'?av.localeCompare(bv):Number(bv||0)-Number(av||0)});document.querySelector('#ranking tbody').innerHTML=rows.map(function(r,i){return '<tr><td class="'+scoreClass(r.overall)+'">'+r.overall.toFixed(1)+'%</td><td>'+esc(r.alias)+'</td><td class="'+scoreClass(r.easy)+'">'+r.easy.toFixed(1)+'%</td><td class="'+scoreClass(r.medium)+'">'+r.medium.toFixed(1)+'%</td><td class="'+scoreClass(r.hard)+'">'+r.hard.toFixed(1)+'%</td><td class="'+scoreClass(r.fullPass)+'">'+r.fullPass.toFixed(1)+'%</td><td>'+(r.speed?r.speed.toFixed(1):'—')+'</td><td>'+(r.duration?r.duration.toFixed(2)+'s':'—')+'</td></tr>'}).join('')||'<tr><td colspan="8" class="empty">No professional benchmark results.</td></tr>'}
function renderScatter(){const svg=document.getElementById('scatterChart'),rows=aggregates('all'),w=1200,h=430,p={l:72,r:30,t:28,b:60},cw=w-p.l-p.r,ch=h-p.t-p.b,xMax=Math.max(1,...rows.map(function(r){return r.speed||0}))*1.1;let out='';for(let i=0;i<=4;i++){const y=p.t+ch-i*ch/4;out+='<line x1="'+p.l+'" x2="'+(w-p.r)+'" y1="'+y+'" y2="'+y+'" stroke="#3a4555" stroke-dasharray="3 4"/><text x="'+(p.l-10)+'" y="'+(y+4)+'" text-anchor="end" fill="#9aa8ba" font-size="12">'+(i*25)+'</text>'}out+='<line x1="'+p.l+'" x2="'+(w-p.r)+'" y1="'+(p.t+ch)+'" y2="'+(p.t+ch)+'" stroke="#9aa8ba"/><text x="'+(p.l+cw/2)+'" y="'+(h-14)+'" text-anchor="middle" fill="#9aa8ba" font-size="13">Completion tok/s</text><text x="18" y="'+(p.t+ch/2)+'" transform="rotate(-90 18 '+(p.t+ch/2)+')" text-anchor="middle" fill="#9aa8ba" font-size="13">Score (%)</text>';rows.forEach(function(r){if(!r.speed)return;const x=p.l+(r.speed/xMax)*cw,y=p.t+ch-(r.overall/100)*ch;out+='<circle cx="'+x+'" cy="'+y+'" r="8" fill="'+modelColor.get(r.alias)+'"><title>'+esc(r.alias)+': '+r.overall.toFixed(1)+'% at '+r.speed.toFixed(1)+' tok/s</title></circle><text x="'+(x+11)+'" y="'+(y+4)+'" fill="#ecf1f8" font-size="11">'+esc(r.alias)+'</text>'});svg.innerHTML=out}
function renderReliability(){const rows=models.map(function(model){const all=professional.filter(function(r){return r.alias===model}),attempts=new Map(),expected=new Set(all.map(function(r){return r.benchmarkId})).size;all.forEach(function(r){const k=String(r.attempt);if(!attempts.has(k))attempts.set(k,[]);attempts.get(k).push(r)});const values=all.map(function(r){return r.score}).filter(function(v){return v!=null}),avg=mean(values)||0,variance=mean(values.map(function(v){return (v-avg)*(v-avg)}))||0,stability=Math.max(0,100-Math.sqrt(variance));const full=[...attempts.values()].filter(function(a){return a.length>=expected&&a.every(function(r){return r.score===100})}).length;return {model:model,attempts:attempts.size,full:attempts.size?full/attempts.size*100:0,avg:avg,stability:stability}}).sort(function(a,b){return b.stability-a.stability});document.querySelector('#reliability tbody').innerHTML=rows.map(function(r){return '<tr><td>'+esc(r.model)+'</td><td>'+r.attempts+'</td><td class="'+scoreClass(r.full)+'">'+r.full.toFixed(1)+'%</td><td class="'+scoreClass(r.avg)+'">'+r.avg.toFixed(1)+'%</td><td class="'+scoreClass(r.stability)+'">'+r.stability.toFixed(1)+'%</td></tr>'}).join('')||'<tr><td colspan="5" class="empty">No professional benchmark results.</td></tr>'}
function renderDetails(){const list=DATA.filter(function(r){return customShown||r.benchmarkKind==='professional'});document.getElementById('details').innerHTML=list.map(function(r){return '<details><summary><span style="color:'+modelColor.get(r.alias)+'">●</span> '+esc(r.alias)+' · '+esc(r.benchmarkTitle)+' · attempt '+r.attempt+' · '+(r.score!=null?'<span class="'+scoreClass(r.score)+'">'+r.score.toFixed(1)+'%</span>':'<span>ungraded</span>')+'</summary><div class="detail-grid"><div><span>Status</span><b>'+esc(r.status)+'</b></div><div><span>Tests</span><b>'+(r.passed!=null?r.passed+'/'+r.total:'—')+'</b></div><div><span>Tokens/sec</span><b>'+(r.tokensPerSecond!=null?r.tokensPerSecond.toFixed(1):'—')+'</b></div><div><span>Duration</span><b>'+(r.durationSeconds!=null?r.durationSeconds.toFixed(2)+'s':'—')+'</b></div></div>'+(r.feedback&&r.feedback.length?'<ul class="feedback">'+r.feedback.map(function(f){return '<li>'+esc(f)+'</li>'}).join('')+'</ul>':'')+'<p style="margin-top:9px;font-size:11px;color:#9aa8ba">Artifact: '+esc(r.outputPath)+'</p></details>'}).join('')||'<div class="empty">No result details.</div>'}
function renderAll(){renderCards();renderBars('accuracyChart','accuracyLegend',professional,'score',['easy','medium','hard','overall'],100,'Score (%)');renderBars('speedChart','speedLegend',professional,'tokensPerSecond',['easy','medium','hard'],Math.max(1,...professional.map(function(r){return r.tokensPerSecond||0})),'Completion tok/s');renderScatter();renderRanking();renderReliability();renderDetails()}
document.getElementById('difficulty').onchange=renderRanking;document.getElementById('toggleCustom').onclick=function(){customShown=!customShown;this.textContent=customShown?'Hide custom prompt performance':'Show custom prompt performance';renderDetails()};document.querySelectorAll('th[data-key]').forEach(function(th){th.onclick=function(){sortKey=th.dataset.key;renderRanking()}});renderAll();
</script>
</body>
</html>
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_self_contained_report_and_summary() {
        let dir = std::env::temp_dir().join(format!(
            "llama-switcher-benchmark-report-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let records = vec![BenchmarkReportRecord {
            profile_id: "profile".into(),
            alias: "Model <one>".into(),
            benchmark_id: "case".into(),
            benchmark_title: "Case".into(),
            benchmark_kind: "professional".into(),
            difficulty: Some("easy".into()),
            weight: Some(0.0333333333),
            attempt: 1,
            status: "passed".into(),
            score: Some(100.0),
            passed: Some(1),
            total: Some(1),
            duration_seconds: Some(1.0),
            tokens_per_second: Some(10.0),
            draft_tokens: None,
            accepted_draft_tokens: None,
            speculative_acceptance_rate: None,
            output_path: dir.join("artifact").to_string_lossy().to_string(),
            feedback: vec!["safe <text>".into()],
        }];
        let report = write_html_report(&dir, &records).unwrap();
        let html = std::fs::read_to_string(report).unwrap();
        assert!(html.contains("<svg"));
        assert!(html.contains("Model \\u003cone\\u003e"));
        assert!(!html.contains("Model <one>"));
        assert!(!html.contains("__REPORT_DATA__"));
        let script_start = html.find("<script>").unwrap() + "<script>".len();
        let script_end = html.rfind("</script>").unwrap();
        let wrapped_script = format!(
            "function __validateGeneratedReport() {{\n{}\n}}",
            &html[script_start..script_end]
        );
        let mut context = boa_engine::Context::default();
        context
            .eval(boa_engine::Source::from_bytes(wrapped_script.as_bytes()))
            .expect("generated report JavaScript should parse");
        assert!(dir.join("summary.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
