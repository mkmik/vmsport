$! Files vmsport wrote: checked, read by key, dumped in every key's order;
$! VMS goes on writing to RT1. "@@" lines on SYS$ERROR say how far it got.
$ SET NOON
$ i = 0
$loop:
$ name = F$ELEMENT(i, ",", "RT1,RT2,RT3,RT4,RT5,RT6,RT7")
$ IF name .EQS. "," THEN GOTO look
$ WRITE SYS$ERROR "@@ analyze ", name
$ ANALYZE/RMS_FILE/CHECK/OUTPUT='name'.CHK 'name'.DAT
$ ANALYZE/RMS_FILE/FDL/OUTPUT='name'.ANL 'name'.DAT
$ i = i + 1
$ GOTO loop
$look:
$ WRITE SYS$ERROR "@@ look"
$ k = 1
$l1:
$ kk = F$FAO("ID!6ZL", k)
$ @LOOK RT1.DAT 0 'kk'
$ k = k + 23
$ IF k .LE. 600 THEN GOTO l1
$ @LOOK RT1.DAT 1 MOVED
$ @LOOK RT1.DAT 1 Q
$ @LOOK RT1.DAT 2 0010
$ @LOOK RT1.DAT 3 M022
$ k = 1
$l2:
$ kk = F$FAO("A!3ZLB!2ZL", k, k - (k / 100) * 100)
$ @LOOK RT2.DAT 0 'kk'
$ k = k + 37
$ IF k .LE. 900 THEN GOTO l2
$ @LOOK RT2.DAT 1 PARIS
$ @LOOK RT2.DAT 1 ROME
$ k = 1
$l3:
$ kk = F$FAO("R!3ZL", k)
$ @LOOK RT3.DAT 0 'kk'
$ k = k + 17
$ IF k .LE. 306 THEN GOTO l3
$ k = 1
$l4:
$ kk = F$FAO("K!5ZL", k)
$ @LOOK RT4.DAT 0 'kk'
$ k = k + 11
$ IF k .LE. 210 THEN GOTO l4
$ @LOOK RT5.DAT 0 AAA
$ @LOOK RT5.DAT 0 BBB
$ @LOOK RT5.DAT 0 DDD
$ @LOOK RT7.DAT 0 K00001
$!
$ WRITE SYS$ERROR "@@ rt1v"
$ COPY RT1.DAT RT1V.DAT
$ cities = "ROME,PARIS,ZAGREB,OSLO,LIMA,TOKYO,QUITO"
$ OPEN/READ/WRITE f RT1V.DAT
$ n = 1
$v1:
$ k = 601 + n * 37 - (n * 37 / 101) * 101
$ GOSUB idxrec
$ WRITE f rec
$ n = n + 1
$ IF n .LE. 100 THEN GOTO v1
$ k = 2
$v2:
$ IF k - (k / 13) * 13 .EQ. 0 THEN GOTO v3
$ id = F$FAO("ID!6ZL", k)
$ READ/DELETE/KEY='id' f rec
$v3:
$ k = k + 2
$ IF k .LE. 40 THEN GOTO v2
$ CLOSE f
$ ANALYZE/RMS_FILE/CHECK/OUTPUT=RT1V.CHK RT1V.DAT
$ i = 0
$dump:
$ e = F$ELEMENT(i, ",", "RT1/3,RT3/8,RT4/0,RT5/1,RT6/1,RT7/0,RT2/1")
$ IF e .EQS. "," THEN GOTO done
$ name = F$ELEMENT(0, "/", e)
$ last = F$INTEGER(F$ELEMENT(1, "/", e))
$ j = 0
$keys:
$ WRITE SYS$ERROR "@@ convert ", name, " ", j
$ CONVERT/KEY='j'/TRUNCATE/FDL=SEQ16.FDL 'name'.DAT 'name''j'.SEQ
$ j = j + 1
$ IF j .LE. last THEN GOTO keys
$ i = i + 1
$ GOTO dump
$done:
$ WRITE SYS$ERROR "@@ done"
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
