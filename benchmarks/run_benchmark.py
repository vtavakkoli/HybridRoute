#!/usr/bin/env python3
import html, json, os, statistics, time
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

BASE=os.getenv("HYBRIDROUTE_URL","http://hybridroute:8080").rstrip("/")
TOKEN=os.getenv("ADMIN_TOKEN","benchmark-secret")
RESULTS=Path("/work/results"); RESULTS.mkdir(parents=True, exist_ok=True)

INTENTS = {
"streetlight-report": (["broken streetlight outside my home","public lamp keeps blinking","street lighting is not working","dark road because a lamp failed","damaged streetlight pole","report an unlit public lamp","street lamp switched off","lighting failure on the avenue","replace a broken road lamp","night street is dark"], "infrastructure"),
"parking-permit": (["renew my residential parking permit","apply for a parking permit","change address on parking permit","cancel resident parking authorization","parking permit expired","new resident permit for my car","update vehicle on permit","parking authorization renewal","manage my district parking permit","request residential parking card"], "mobility"),
"invoice-processing": (["process supplier invoice INV-100","validate invoice number and amount","archive this billing document","classify a supplier invoice","extract invoice amount","check invoice INV-200","submit vendor billing record","process accounts payable invoice","validate supplier bill","store invoice metadata"], "finance"),
"waste-collection": (["garbage was not collected","request a recycling bin","missed waste pickup","replace a damaged rubbish bin","schedule bulky waste collection","report overflowing garbage container","recycling collection problem","order a bio waste bin","trash pickup did not arrive","manage household waste collection"], "environment"),
"water-leak": (["water pipe is leaking","report a burst water main","flooding from public pipe","no water supply in building","leak under the street","broken water connection","water main failure","report continuous water loss","pipe burst near sidewalk","utility water leak"], "utilities"),
"housing-support": (["need help paying rent","apply for social housing","request housing benefit","rent assistance application","emergency housing support","subsidized apartment request","financial support for rent","update social housing case","housing allowance question","apply for municipal housing"], "housing"),
"library-service": (["renew my library book","reserve a book at library","replace library card","extend book loan","find borrowed books","cancel library reservation","manage library membership","book return deadline","request a new library card","reserve reading material"], "culture"),
"public-transport": (["my tram is delayed","find a public transport route","bus did not arrive","metro service interruption","train connection information","public transport ticket question","report tram delay","plan bus journey","underground line disruption","mobility route by metro"], "mobility"),
"event-permit": (["apply for street event permit","organize a public festival","authorization for outdoor market","permit for community event","register a street gathering","public concert approval","festival permit application","organize neighborhood market","event authorization request","permit for public celebration"], "administration"),
"document-summarization": (["summarize this report","extract key points from document","shorten this long text","create an executive summary","summarize attached document","identify main findings","condense the report","make a short document overview","produce bullet summary","explain key points briefly"], "ai"),
}
CONTEXTS=[
("basic",[]), ("citizen",["citizen"]), ("agent",["service-agent"]), ("mobile",["citizen","mobile"]), ("portal",["citizen","portal"]),
("english",["citizen","lang-en"]), ("priority",["service-agent","priority"]), ("tenant-a",["citizen","tenant-a"]), ("tenant-b",["citizen","tenant-b"]), ("audited",["service-agent","audited"]),
]

def scenarios():
    output=[]; number=1
    for expected,(phrases,domain) in INTENTS.items():
        for phrase in phrases:
            for context_name,roles in CONTEXTS:
                body={"query":phrase,"context":context_name}
                if expected=="invoice-processing": body.update(invoice_number=f"INV-{number:04d}",amount=round(10+number/10,2))
                output.append({"id":f"S{number:04d}","expected":expected,"text":phrase,"domain":domain,"roles":roles,"body":body})
                number+=1
    assert len(output)==1000
    return output

def request_json(path,payload=None,headers=None,method="POST",timeout=15):
    data=None if payload is None else json.dumps(payload).encode()
    h={"content-type":"application/json"}; h.update(headers or {})
    req=Request(BASE+path,data=data,headers=h,method=method)
    try:
        with urlopen(req,timeout=timeout) as response: return response.status, json.loads(response.read() or b"{}")
    except HTTPError as error:
        raw=error.read();
        try: body=json.loads(raw or b"{}")
        except Exception: body={"error":raw.decode(errors="replace")}
        return error.code,body

def percentile(values,p):
    if not values:return 0.0
    values=sorted(values); index=min(len(values)-1,max(0,int(round((len(values)-1)*p))))
    return values[index]

def feature_checks():
    checks=[]
    status,health=request_json("/healthz",method="GET"); checks.append(("health",status==200 and health.get("routes",0)>=11,str(health)))
    try:
        with urlopen(BASE+"/metrics",timeout=10) as response: metrics=response.read().decode(); ok="hybridroute_decisions_total" in metrics
    except Exception as exc: metrics=str(exc); ok=False
    checks.append(("metrics",ok,metrics[:160]))
    before=health.get("generation",0); status,reload=request_json("/v1/admin/reload",{}, {"x-hybridroute-admin-token":TOKEN}); checks.append(("atomic_reload",status==200 and reload.get("generation",0)>before,str(reload)))
    status,adapt=request_json("/v1/feedback",{"route_id":"streetlight-report","reward":1.0,"success":True},{"x-hybridroute-admin-token":TOKEN}); checks.append(("safe_adaptation",status==200 and adapt.get("accepted") is True,str(adapt)))
    status,blocked=request_json("/v1/feedback",{"route_id":"parking-permit","reward":1.0,"success":True},{"x-hybridroute-admin-token":TOKEN}); checks.append(("high_impact_adaptation_blocked",status>=400,str(blocked)))
    status,schema=request_json("/v1/route",{"text":"process this invoice","method":"POST","content_type":"application/json","domain":"finance","body":{"query":"process this invoice"}}); selected=(schema.get("selected") or {}).get("route_id"); checks.append(("schema_filter",status==200 and selected!="invoice-processing",str(schema)[:200]))
    status,amb=request_json("/v1/route",{"text":"parking event permit application","method":"POST","content_type":"application/json","body":{"query":"parking event permit application"}}); checks.append(("clarification_mode",status==200 and amb.get("mode") in {"clarification","top_score","confident"},str(amb)[:200]))
    return checks

def main():
    corpus=scenarios(); (RESULTS/"scenarios.jsonl").write_text("".join(json.dumps(x)+"\n" for x in corpus))
    rows=[]; latencies=[]; errors=0; modes=Counter(); confusion=Counter(); started=time.perf_counter()
    for case in corpus:
        payload={"text":case["text"],"method":"POST","content_type":"application/json","domain":case["domain"],"roles":case["roles"],"body":case["body"],"sticky_key":case["id"],"top_k":5}
        t0=time.perf_counter()
        try: status,decision=request_json("/v1/route",payload); error=None
        except Exception as exc: status=0; decision={}; error=str(exc); errors+=1
        latency=(time.perf_counter()-t0)*1000; latencies.append(latency)
        selected=(decision.get("selected") or {}).get("route_id"); candidates=[c.get("route_id") for c in decision.get("candidates",[])]; mode=decision.get("mode","error"); modes[mode]+=1
        correct=selected==case["expected"]; top3=case["expected"] in candidates[:3]; confusion[(case["expected"],selected or mode)]+=1
        rows.append({**case,"status":status,"selected":selected,"correct":correct,"top3":top3,"mode":mode,"latency_ms":round(latency,3),"error":error})
    elapsed=time.perf_counter()-started; checks=feature_checks(); correct=sum(r["correct"] for r in rows); top3=sum(r["top3"] for r in rows)
    summary={"generated_at":datetime.now(timezone.utc).isoformat(),"scenarios":len(rows),"correct":correct,"accuracy":correct/len(rows),"top3_accuracy":top3/len(rows),"errors":errors,"duration_seconds":elapsed,"throughput_per_second":len(rows)/elapsed if elapsed else 0,"latency_ms":{"mean":statistics.fmean(latencies),"p50":percentile(latencies,.50),"p95":percentile(latencies,.95),"p99":percentile(latencies,.99),"max":max(latencies)},"modes":dict(modes),"feature_checks":[{"name":n,"passed":p,"detail":d} for n,p,d in checks]}
    (RESULTS/"benchmark-summary.json").write_text(json.dumps(summary,indent=2)); (RESULTS/"scenario-results.jsonl").write_text("".join(json.dumps(r)+"\n" for r in rows))
    build_html(summary,rows,confusion)
    print(json.dumps(summary,indent=2))
    if errors or not all(p for _,p,_ in checks): raise SystemExit(1)

def build_html(summary,rows,confusion):
    per=defaultdict(lambda:[0,0])
    for r in rows: per[r["expected"]][0]+=1; per[r["expected"]][1]+=int(r["correct"])
    feature="".join(f"<tr><td>{html.escape(c['name'])}</td><td class={'ok' if c['passed'] else 'bad'}>{'PASS' if c['passed'] else 'FAIL'}</td><td>{html.escape(c['detail'])}</td></tr>" for c in summary["feature_checks"])
    categories="".join(f"<tr><td>{html.escape(k)}</td><td>{v[0]}</td><td>{v[1]}</td><td>{v[1]/v[0]:.2%}</td></tr>" for k,v in sorted(per.items()))
    failed_rows=[r for r in rows if not r["correct"]][:100]
    failures="".join(f"<tr><td>{r['id']}</td><td>{html.escape(r['expected'])}</td><td>{html.escape(str(r['selected']))}</td><td>{html.escape(r['text'])}</td></tr>" for r in failed_rows)
    report=f'''<!doctype html><html><head><meta charset="utf-8"><title>HybridRoute Benchmark Report</title><style>body{{font-family:system-ui;margin:32px;background:#f5f7fb;color:#18202b}}.cards{{display:flex;gap:12px;flex-wrap:wrap}}.card{{background:white;padding:18px;border-radius:12px;box-shadow:0 2px 10px #0001;min-width:150px}}table{{width:100%;border-collapse:collapse;background:white;margin:18px 0}}th,td{{padding:9px;border-bottom:1px solid #ddd;text-align:left}}.ok{{color:#087830;font-weight:bold}}.bad{{color:#b00020;font-weight:bold}}code{{background:#eef;padding:2px 4px}}</style></head><body><h1>HybridRoute v0.2 Benchmark</h1><p>Generated {summary['generated_at']} by <code>docker compose up --build --exit-code-from test test</code>.</p><div class="cards"><div class="card"><b>Scenarios</b><br>{summary['scenarios']}</div><div class="card"><b>Accuracy</b><br>{summary['accuracy']:.2%}</div><div class="card"><b>Top-3</b><br>{summary['top3_accuracy']:.2%}</div><div class="card"><b>p95</b><br>{summary['latency_ms']['p95']:.2f} ms</div><div class="card"><b>Throughput</b><br>{summary['throughput_per_second']:.1f}/s</div></div><h2>Feature checks</h2><table><tr><th>Check</th><th>Status</th><th>Detail</th></tr>{feature}</table><h2>Accuracy by intent</h2><table><tr><th>Intent</th><th>Total</th><th>Correct</th><th>Accuracy</th></tr>{categories}</table><h2>First failures</h2><table><tr><th>ID</th><th>Expected</th><th>Selected</th><th>Text</th></tr>{failures or '<tr><td colspan=4>No failures</td></tr>'}</table></body></html>'''
    (RESULTS/"benchmark-report.html").write_text(report)

if __name__=="__main__": main()
