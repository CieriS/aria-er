"""Throwaway exploration of ARPAE samples (phase 0)."""
import csv, glob, json, collections, datetime as dt
rows=[]
for f in sorted(glob.glob('data/samples/arpae/storico_2025/*.csv')):
    with open(f, newline='', encoding='utf-8') as fh:
        rd=csv.DictReader(fh); hdr=rd.fieldnames
        for r in rd: r['_file']=f.split('/')[-1]; rows.append(r)
print('header', hdr, 'rows', len(rows))
print('UM', collections.Counter(r['UM'] for r in rows))
vals=collections.Counter()
for r in rows:
    try: float(r['VALORE'])
    except ValueError: vals[r['VALORE']]+=1
print('non-numeric VALORE', vals.most_common(10))
p=lambda s: dt.datetime.strptime(s,'%d/%m/%Y %H')
dur=collections.Counter((r['ID_PARAM'],(p(r['DATA_FINE'])-p(r['DATA_INIZIO'])).total_seconds()/3600) for r in rows)
print('duration by param', sorted(dur.items()))
key=collections.Counter((r['COD_STAZ'],r['ID_PARAM'],r['DATA_INIZIO']) for r in rows)
d=[k for k,c in key.items() if c>1]; print('dup keys', len(d), d[:5])
for k in d[:3]: print([ (r['DATA_INIZIO'],r['DATA_FINE'],r['VALORE']) for r in rows if (r['COD_STAZ'],r['ID_PARAM'],r['DATA_INIZIO'])==k])
# DST: rows around 30/03 and 26/10
for day in ('30/03/2025','26/10/2025'):
    print(day, [(r['DATA_INIZIO'],r['DATA_FINE'],r['VALORE']) for r in rows if r['_file']=='storico_2025_07000014_008.csv' and r['DATA_INIZIO'].startswith(day)][:6])
# completeness
exp={1440:365,60:8760}
bycount=collections.Counter(r['_file'] for r in rows)
for f,c in sorted(bycount.items()):
    par=f.split('_')[3][:3]; step=1440 if par in('005','111') else 60
    print(f, c, f'{100*c/exp[step]:.1f}%')
# anomalies
num=[(float(r['VALORE']),r) for r in rows if r['VALORE'].replace('.','',1).lstrip('-').isdigit()]
print('negatives', [(v,r['_file'],r['DATA_INIZIO']) for v,r in num if v<0][:10], sum(v<0 for v,_ in num))
print('zeros', collections.Counter(r['_file'] for v,r in num if v==0).most_common(5))
for par in ('005','008','010'):
    s=sorted(((v,r['_file'],r['DATA_INIZIO']) for v,r in num if r['_file'].endswith(par+'.csv')),reverse=True)[:5]; print('max',par,s)
# decimal formats
print('decimals', collections.Counter(('.' in r['VALORE'], r['ID_PARAM']) for r in rows))
# gaps: longest run of missing hours per hourly file
for f in sorted(bycount):
    if f.endswith(('005.csv','111.csv')): continue
    ts=sorted(p(r['DATA_INIZIO']) for r in rows if r['_file']==f)
    g=max(((b-a),a) for a,b in zip(ts,ts[1:]))
    print('max gap',f,g[0],g[1])
nrt=json.load(open('data/samples/arpae/nrt_bologna.json'))['result']['records']
print('NRT flags per param', collections.Counter((x['variable_id'],x['v_flag']) for x in nrt))
print('NRT reftime range', min(x['reftime'] for x in nrt), max(x['reftime'] for x in nrt))
k=collections.Counter((x['station_id'],x['variable_id'],x['reftime']) for x in nrt); print('NRT dups', sum(c>1 for c in k.values()))
print('NRT values nonnum', collections.Counter(x['value'] for x in nrt if not x['value'].replace('.','',1).lstrip('-').isdigit()))
print('NRT daily param hours', collections.Counter(x['reftime'][-5:] for x in nrt if x['variable_id'] in('5','111')))
