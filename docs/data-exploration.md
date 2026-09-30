# Fase 0 — Esplorazione dati ARPAE

Campione analizzato: anno 2025, stazioni del Comune di Bologna. Script: `scratch/explore.py` (usa-e-getta).
Dati in `data/samples/arpae/`.

## 1. Sorgenti verificate

Dataset CKAN: `qualita-dell-aria-rete-di-monitoraggio`
(`https://dati.arpae.it/api/3/action/package_show?id=qualita-dell-aria-rete-di-monitoraggio`)

| Risorsa | ID / URL | Contenuto |
|---|---|---|
| Datastore NRT | `4dc855a1-6298-4b71-a1ae-d80693d43dcb` — `https://dati.arpae.it/api/3/action/datastore_search_sql` | Ultime ~7 settimane (al 30/09/2026: 28/07 → 17/09/2026) |
| Storico dal 2010 | Cartella Google Drive `1nBPCq1laGFCJZhbunV3yLHciZky0JcBi` (resourcekey `0-SxZAhXpvnVSBVJjG_HYZ_w`) | 3.613 CSV `storico_<anno>_<staz8>_<param3>.csv`, 2010–2025 |
| Anagrafe stazioni | `https://docs.google.com/spreadsheets/d/1-4wgZ8JeLeg0bODTSFUrshPY-_y9mERUu0FJtSFr78s/export?format=csv` | Una riga per stazione×parametro |
| Anagrafe parametri | `https://docs.google.com/spreadsheets/d/1K6vRcShjje2CDnvnk39o3jkU4ihgpA7rfsdAV1S-yXU/export?format=csv` | ID, nome, unità, periodo di mediazione |

Note:
- `datastore_search` sulla risorsa NRT restituisce **HTTP 500**; `datastore_search_sql` funziona.
- Lo storico **non è sul datastore CKAN** ma su Google Drive: i file si scaricano con
  `https://drive.google.com/uc?export=download&id=<file_id>`, l'elenco si ottiene dalla pagina
  `drive.google.com/embeddedfolderview?id=...` (HTML, non un'API stabile).
- L'anno 2026 non è ancora nello storico: 2026 fino a fine luglio non è coperto da nessuna delle due sorgenti
  nel campione osservato (gap tra storico e finestra NRT).
- Esiste anche un file spurio `test_storico_2025_3000022_7.csv` nella cartella.

Stazioni di Bologna comune in anagrafe: 7.000.014 Giardini Margherita (fondo), 7.000.015 Porta San Felice
(traffico), 7.000.041 Via Chiarini, 7.000.047 Cabina Mainsite (rete ricerca; **nessun file 2025** nello storico).
14 file scaricati, 79.240 righe.

## 2. Schema reale

### Storico CSV
```
COD_STAZ,ID_PARAM,DATA_INIZIO,DATA_FINE,VALORE,UM
7000014,8,01/01/2025 00,01/01/2025 01,6,ug/m3
7000014,5,01/01/2025 00,02/01/2025 00,32,ug/m3
```
| Colonna | Tipo | Note |
|---|---|---|
| COD_STAZ | intero (7 cifre, zero iniziale perso) | nel nome file è a 8 cifre (`07000014`), in anagrafe `7.000.014` |
| ID_PARAM | intero | 5=PM10, 7=O3, 8=NO2, 9=NOx, 10=CO, 20=benzene, 38=NO, 111=PM2.5 |
| DATA_INIZIO / DATA_FINE | `DD/MM/YYYY HH` | intervallo di mediazione: 1h (orari) o 24h (PM10, PM2.5) |
| VALORE | decimale con `.` | interi per la maggior parte dei parametri; decimali per CO e benzene |
| UM | testo | `ug/m3`, `mg/m3` (CO). Nessuna conversione necessaria per µg/m³ tranne CO (×1000) |

Nessun flag di validità nello storico (si presume tutto validato).

### Datastore NRT
Campi (tutti `text`): `station_id` (`7000014`), `variable_id` (`8`), `reftime` (`MM/DD/YYYY HH:MM`),
`value`, `v_flag` (`G` / `M`). Nessuna unità: va presa dall'anagrafe parametri.

### Timestamp e timezone
- Nessuna indicazione esplicita di timezone.
- Nei giorni di cambio ora (30/03/2025 e 26/10/2025) ci sono **esattamente 24 ore** e l'ora `02` del 30/03
  (inesistente in ora legale italiana) è presente → gli orari sono in **offset fisso**, non in ora civile
  Europe/Rome. Ipotesi più probabile: ora solare UTC+1 (convenzione standard delle reti QA). **Da confermare
  con ARPAE** prima di convertire in UTC.
- Anagrafe parametri: per gli orari "l'ora riportata è quella di fine rilevazione". Nello storico la fine è
  `DATA_FINE`; nell'NRT `reftime` va quindi interpretato come fine intervallo (da verificare incrociando
  i periodi di sovrapposizione quando lo storico 2026 sarà pubblicato).
- Formati data diversi: storico `DD/MM`, NRT `MM/DD`.

## 3. Chiave naturale
`(station_id, pollutant_id, period_start)` — nessun duplicato nel campione (storico e NRT).
In storage conviene includere anche l'intervallo di mediazione (1h/24h) come attributo, perché PM10/PM2.5
giornalieri e i parametri orari condividono la stessa tabella.

## 4. Problemi di qualità osservati

1. **Righe mancanti (buchi temporali), non esplicitate.** Le ore mancanti non sono righe con valore nullo:
   semplicemente non esistono. Completezza 2025: da 94,1% (benzene P.S. Felice, 8.239/8.760) a 99,5%.
   Buchi massimi: NO2 Giardini Margherita 4 giorni e 10 ore dal 30/10/2025 02:00; benzene P.S. Felice 4 giorni
   dal 24/02/2025 10:00; interruzione simultanea di NO/NO2/NOx/CO a P.S. Felice il 06/05/2025 (~23h),
   compatibile con un guasto di cabina. → la completezza va calcolata contro una griglia attesa.
2. **Dati provvisori vs validati nell'NRT.** `v_flag` assume `M` (68% dei record Bologna) e `G` (32%);
   il significato non è documentato nella risorsa (ipotesi: M = misura grezza/non validata, G = validata).
   Lo stesso istante può quindi cambiare valore nel tempo → serve upsert sulla finestra mobile, e il flag va
   conservato in raw.
3. **Identificativi e formati incoerenti fra le risorse.** Codice stazione in 3 formati (`7000014`,
   `07000014`, `7.000.014`); date `DD/MM/YYYY HH` vs `MM/DD/YYYY HH:MM`; unità presente solo nello storico;
   CO in mg/m³ mentre tutto il resto in µg/m³.
4. **Zeri sospetti.** Nessun valore negativo o non numerico, ma molti zeri esatti: 242 per NO a P.S. Felice,
   110 per O3 a Giardini Margherita, 108 per CO (probabilmente sotto soglia di rilevabilità + arrotondamento
   all'intero). Non vanno scartati ma flaggati/documentati.
5. **Precisione arrotondata e mista.** La maggior parte dei parametri è intera; CO e benzene hanno decimali
   ma 359/8.714 valori CO sono interi. Non è un errore, ma leggere sempre come decimale.
6. **Sorgente storico fragile.** Google Drive senza API CKAN, un file per anno×stazione×parametro, file di test
   nella stessa cartella, stazioni presenti in anagrafe ma senza file (7.000.047 nel 2025).

## 5. Implicazioni per la fase 1 (da discutere)
- L'ingestor incrementale (finestra 30 gg) deve leggere l'**NRT via `datastore_search_sql`**; lo storico
  Drive serve solo per il backfill.
- Conservare in raw valore originale, unità, `v_flag` e il timestamp originale; la conversione a UTC dipende
  dalla conferma della timezone (punto 2).
