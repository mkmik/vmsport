$ SET NOON
$ say = "WRITE SYS$OUTPUT"
$ cities = "ROME,PARIS,ZAGREB,OSLO,LIMA,TOKYO,QUITO"
$ pad = F$FAO("!130*v")
$!
$! Empty files, as CREATE/FDL makes them
$ CREATE/FDL=IDX.FDL E1.DAT
$ CREATE/FDL=IDXVAR.FDL E2.DAT
$ CREATE/FDL=TY.FDL E3.DAT
$ CREATE/FDL=KS.FDL E4.DAT
$ CREATE/FDL=ML.FDL E5.DAT
$ CREATE/FDL=NC.FDL E6.DAT
$ CREATE/FDL=PDUP.FDL E7.DAT
$!
$! One put, two puts
$ CREATE/FDL=IDX.FDL P1.DAT
$ OPEN/READ/WRITE f P1.DAT
$ k = 37
$ GOSUB idxrec
$ WRITE f rec
$ CLOSE f
$ CREATE/FDL=IDX.FDL P2.DAT
$ OPEN/READ/WRITE f P2.DAT
$ k = 37
$ GOSUB idxrec
$ WRITE f rec
$ k = 74
$ GOSUB idxrec
$ WRITE f rec
$ CLOSE f
$!
$! RB: fixtures/rms IDX.DAT again
$ CREATE/FDL=IDX.FDL RB.DAT
$ OPEN/READ/WRITE f RB.DAT
$ n = 1
$rb:
$ k = n * 37 - (n * 37 / 151) * 151
$ GOSUB idxrec
$ WRITE f rec
$ n = n + 1
$ IF n .LE. 150 THEN GOTO rb
$ READ/DELETE/KEY="ID000010" f rec
$ READ/DELETE/KEY="ID000011" f rec
$ READ/DELETE/KEY="ID000100" f rec
$ READ/KEY="ID000020" f rec
$ rec = F$EXTRACT(0, 8, rec) + "NEWCITY   " + F$EXTRACT(18, 46, rec)
$ WRITE/UPDATE f rec
$ CLOSE f
$!
$! AS, DS: ascending and descending loads
$ CREATE/FDL=IDXVAR.FDL AS.DAT
$ OPEN/READ/WRITE f AS.DAT
$ k = 1
$as:
$ WRITE f F$FAO("K!5ZL", k) + F$EXTRACT(0, k * 2, pad)
$ k = k + 1
$ IF k .LE. 60 THEN GOTO as
$ CLOSE f
$ CREATE/FDL=IDXVAR.FDL DS.DAT
$ OPEN/READ/WRITE f DS.DAT
$ k = 60
$ds:
$ WRITE f F$FAO("K!5ZL", k) + F$EXTRACT(0, k * 2, pad)
$ k = k - 1
$ IF k .GE. 1 THEN GOTO ds
$ CLOSE f
$!
$! DEL: SIDR elements deleted first, middle, last, all; a key changed
$ CREATE/FDL=IDX.FDL DEL.DAT
$ OPEN/READ/WRITE f DEL.DAT
$ k = 1
$del:
$ GOSUB idxrec
$ WRITE f rec
$ k = k + 1
$ IF k .LE. 30 THEN GOTO del
$ READ/DELETE/KEY="ID000007" f rec
$ READ/DELETE/KEY="ID000021" f rec
$ READ/DELETE/KEY="ID000028" f rec
$ READ/DELETE/KEY="ID000004" f rec
$ READ/DELETE/KEY="ID000011" f rec
$ READ/DELETE/KEY="ID000018" f rec
$ READ/DELETE/KEY="ID000025" f rec
$ k = 4
$ GOSUB idxrec
$ WRITE f rec
$ READ/KEY="ID000005" f rec
$ rec = F$EXTRACT(0, 8, rec) + "NEWCITY   " + F$EXTRACT(18, 22, rec) + "N999" + F$EXTRACT(44, 20, rec)
$ WRITE/UPDATE f rec
$ CLOSE f
$!
$! PD: duplicates on the primary key, a chain over buckets
$ CREATE/FDL=PDUP.FDL PD.DAT
$ OPEN/READ/WRITE f PD.DAT
$ i = 1
$pd:
$ key = F$ELEMENT(i - (i / 4) * 4, ",", "AAA,BBB,AAA,CCC")
$ WRITE f key + F$FAO(" !4ZL !4ZL!4ZL!4ZL!4ZL!4ZL", i, i * 3, i * 7, i * 11, i * 13, i * 17)
$ i = i + 1
$ IF i .LE. 60 THEN GOTO pd
$ READ/DELETE/KEY="AAA" f rec
$ READ/KEY="BBB" f rec
$ WRITE/UPDATE f rec
$ READ/KEY="CCC" f rec
$ WRITE/UPDATE f rec + "-longer"
$ CLOSE f
$!
$! KS: a segmented primary key away from the front, variable records,
$! a descending alternate key with a null value, two areas
$ CREATE/FDL=KS.FDL KS.DAT
$ OPEN/READ/WRITE f KS.DAT
$ n = 1
$ks:
$ r = n * 17 - (n * 17 / 41) * 41
$ c = F$ELEMENT(r - (r / 5) * 5, ",", "ROME  ,PARIS ,      ,OSLO  ,LIMA  ")
$ rec = "XX" + F$FAO("B!2ZL", r - (r / 10) * 10) + "-----" + F$FAO("A!3ZL", r) + "......" + c + F$EXTRACT(0, r - (r / 30) * 30, pad)
$ IF r - (r / 8) * 8 .EQ. 0 THEN rec = F$EXTRACT(0, 18, rec)
$ WRITE f rec
$ n = n + 1
$ IF n .LE. 40 THEN GOTO ks
$ READ/KEY="A007B07" f rec
$ WRITE/UPDATE f rec + "longer"
$ READ/KEY="A009B09" f rec
$ WRITE/UPDATE f F$EXTRACT(0, 20, rec) + "ZAGREB" + F$EXTRACT(26, 80, rec)
$ READ/DELETE/KEY="A011B01" f rec
$ CLOSE f
$!
$! NC: no key compression, record compression, key not at the front
$ CREATE/FDL=NC.FDL NC.DAT
$ OPEN/READ/WRITE f NC.DAT
$ n = 1
$nc:
$ r = n * 7 - (n * 7 / 31) * 31
$ WRITE f "abc" + F$FAO("K!4ZL", r) + "xx" + F$FAO("C!3ZL", r - (r / 4) * 4) + "zzzzzzzzzzzz" + F$EXTRACT(0, r, pad)
$ n = n + 1
$ IF n .LE. 30 THEN GOTO nc
$ CLOSE f
$!
$! TY: every key type
$ CREATE/FDL=TY.FDL TY.DAT
$ OPEN/READ/WRITE f TY.DAT
$ n = 1
$ty:
$ r = n * 17 - (n * 17 / 41) * 41
$ rec = F$FAO("R!3ZL!36*x", r)
$ rec[32,16] = r - (r / 9) * 9 - 4
$ rec[48,32] = (r - (r / 11) * 11) * 1000003 - 5000000
$ rec[80,32] = r * 1234567
$ rec[112,32] = r - (r / 3) * 3 - 1
$ rec[144,16] = (r - (r / 7) * 7) * 9000
$ rec[160,32] = r * 7
$ hi = 0
$ IF r - (r / 2) * 2 .EQ. 1 THEN hi = %X80000000
$ rec[192,32] = hi
$ d = (r - (r / 13) * 13) * 111 - 600
$ s = 12
$ IF d .LT. 0 THEN s = 13
$ IF d .LT. 0 THEN d = -d
$ rec[224,8] = (d / 10000) * 16 + (d / 1000 - (d / 10000) * 10)
$ rec[232,8] = (d / 100 - (d / 1000) * 10) * 16 + (d / 10 - (d / 100) * 10)
$ rec[240,8] = (d - (d / 10) * 10) * 16 + s
$ dsv = F$FAO("S!3ZL", r - (r / 6) * 6)
$ rec[31,4] := 'dsv'
$ rec[280,32] = r - (r / 5) * 5 - 2
$ WRITE f rec
$ n = n + 1
$ IF n .LE. 40 THEN GOTO ty
$ CLOSE f
$!
$! ML: a deep index, long uncompressed keys
$ CREATE/FDL=ML.FDL ML.DAT
$ OPEN/READ/WRITE f ML.DAT
$ fill = F$FAO("!94*=")
$ dots = F$FAO("!100*.")
$ n = 1
$ml:
$ k = n * 37 - (n * 37 / 307) * 307
$ WRITE f F$FAO("K!5ZL", k) + fill + dots
$ n = n + 1
$ IF n .LE. 300 THEN GOTO ml
$ CLOSE f
$!
$! Each key's order, by CONVERT
$ i = 0
$dump:
$ e = F$ELEMENT(i, ",", "RB/3,DEL/3,PD/1,KS/1,NC/1,TY/8,ML/0")
$ IF e .EQS. "," THEN GOTO ana
$ name = F$ELEMENT(0, "/", e)
$ last = F$INTEGER(F$ELEMENT(1, "/", e))
$ j = 0
$dumpkey:
$ CONVERT/KEY='j'/FDL=SEQ.FDL 'name'.DAT 'name''j'.SEQ
$ j = j + 1
$ IF j .LE. last THEN GOTO dumpkey
$ i = i + 1
$ GOTO dump
$!
$ana:
$ i = 0
$anaf:
$ f = F$ELEMENT(i, ",", "E1,E2,E3,E4,E5,E6,E7,P1,P2,RB,AS,DS,DEL,PD,KS,NC,TY,ML")
$ IF f .EQS. "," THEN GOTO done
$ ANALYZE/RMS_FILE/FDL/OUTPUT='f'.ANL 'f'.DAT
$ ANALYZE/RMS_FILE/CHECK/OUTPUT='f'.CHK 'f'.DAT
$ i = i + 1
$ GOTO anaf
$done:
$ DIRECTORY/FULL *.DAT
$ EXIT 1
$!
$idxrec:
$ id = F$FAO("ID!6ZL", k)
$ city = F$ELEMENT(k - (k / 7) * 7, ",", cities)
$ a = F$FAO("!4ZL", k / 10)
$ b = F$FAO("!4ZL", k - (k / 3) * 3)
$ num = F$FAO("N!3ZL", k / 2)
$ rec = F$FAO("!8AS!10AS!2*.!4AS!6*.!4AS!6*.!4AS!20*.", id, city, a, b, num)
$ RETURN
