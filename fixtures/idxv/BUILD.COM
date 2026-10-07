$! VMS doing what crates/vms-rms/tests/idx_roundtrip.rs does to RT1, RT2
$! and RT5, from CREATE/FDL: V1, V2, V5.
$ SET NOON
$ cities = "ROME,PARIS,ZAGREB,OSLO,LIMA,TOKYO,QUITO"
$ pad = F$FAO("!30*v")
$ CREATE/FDL=IDX.FDL V1.DAT
$ OPEN/READ/WRITE f V1.DAT
$ n = 1
$v1a:
$ k = n * 97 - (n * 97 / 601) * 601
$ GOSUB idxrec
$ WRITE f rec
$ n = n + 1
$ IF n .LE. 600 THEN GOTO v1a
$ k = 13
$v1b:
$ id = F$FAO("ID!6ZL", k)
$ READ/DELETE/KEY='id' f rec
$ k = k + 13
$ IF k .LE. 600 THEN GOTO v1b
$ k = 11
$v1c:
$ IF k - (k / 13) * 13 .EQ. 0 THEN GOTO v1d
$ id = F$FAO("ID!6ZL", k)
$ READ/KEY='id' f rec
$ rec = F$EXTRACT(0, 8, rec) + "MOVED     " + F$EXTRACT(18, 22, rec) + F$FAO("M!3ZL", k) + F$EXTRACT(44, 20, rec)
$ WRITE/UPDATE f rec
$v1d:
$ k = k + 11
$ IF k .LE. 600 THEN GOTO v1c
$ CLOSE f
$!
$ CREATE/FDL=KS.FDL V2.DAT
$ OPEN/READ/WRITE f V2.DAT
$ n = 1
$v2a:
$ r = n * 367 - (n * 367 / 901) * 901
$ c = ""
$ GOSUB ksrec
$ WRITE f rec
$ n = n + 1
$ IF n .LE. 900 THEN GOTO v2a
$ r = 17
$v2b:
$ id = F$FAO("A!3ZLB!2ZL", r, r - (r / 100) * 100)
$ READ/DELETE/KEY='id' f rec
$ r = r + 17
$ IF r .LE. 900 THEN GOTO v2b
$ r = 19
$v2c:
$ IF r - (r / 17) * 17 .EQ. 0 THEN GOTO v2d
$ IF r - (r / 8) * 8 .EQ. 0 THEN GOTO v2d
$ c = ""
$ GOSUB ksrec
$ IF c .EQS. "ROME  "
$ THEN
$   c = "PARIS "
$ ELSE
$   c = "ROME  "
$ ENDIF
$ GOSUB ksrec
$ rec = rec + "+"
$ id = F$FAO("A!3ZLB!2ZL", r, r - (r / 100) * 100)
$ READ/KEY='id' f old
$ WRITE/UPDATE f rec
$v2d:
$ r = r + 19
$ IF r .LE. 900 THEN GOTO v2c
$ CLOSE f
$!
$ CREATE/FDL=PDUP.FDL V5.DAT
$ OPEN/READ/WRITE f V5.DAT
$ i = 1
$v5a:
$ key = F$ELEMENT(i * 7 - (i * 7 / 5) * 5, ",", "AAA,BBB,CCC,DDD,EEE")
$ WRITE f key + F$FAO(" !4ZL !4ZL!4ZL", i, i * 3, i * 7)
$ i = i + 1
$ IF i .LE. 300 THEN GOTO v5a
$ READ/DELETE/KEY="AAA" f rec
$ READ/DELETE/KEY="CCC" f rec
$ READ/DELETE/KEY="EEE" f rec
$ CLOSE f
$!
$ i = 0
$anaf:
$ f = F$ELEMENT(i, ",", "V1,V2,V5")
$ IF f .EQS. "," THEN GOTO done
$ ANALYZE/RMS_FILE/FDL/OUTPUT='f'.ANL 'f'.DAT
$ ANALYZE/RMS_FILE/CHECK/OUTPUT='f'.CHK 'f'.DAT
$ i = i + 1
$ GOTO anaf
$done:
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
$!
$! KS's record r: with city c, or the one r has when c is "".
$ksrec:
$ IF c .NES. "" THEN GOTO ksr1
$ c = "ROME  "
$ IF r - (r / 2) * 2 .EQ. 1 THEN c = "PARIS "
$ IF r - (r / 10) * 10 .EQ. 0 THEN c = "      "
$ksr1:
$ rec = "XXB" + F$FAO("!2ZL", r - (r / 100) * 100) + "-----A" + F$FAO("!3ZL", r) + "......" + c + F$EXTRACT(0, r - (r / 30) * 30, pad)
$ IF r - (r / 8) * 8 .EQ. 0 THEN rec = F$EXTRACT(0, 18, rec)
$ RETURN
