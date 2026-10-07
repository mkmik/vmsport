$! How full RMS fills a data bucket: nine records, then one more put in
$! the middle (mode 0) or at the end (mode 1) that brings the bucket's
$! free space offset to T. A second bucket means it did not fit.
$ SET NOON
$ pat = "0123456789012345678901234567890123456789012345678901234567890123456789"
$ i = 0
$fdls:
$ fdl = F$ELEMENT(i, ",", "CPK,CPN,CPKR")
$ IF fdl .EQS. "," THEN GOTO done
$ base = F$INTEGER(F$ELEMENT(i, ",", "439,474,469"))
$ m = 0
$modes:
$ t = 495
$ts:
$ CREATE/FDL='fdl'.FDL P.DAT
$ OPEN/READ/WRITE f P.DAT
$ WRITE f "K0000010" + F$EXTRACT(0, 30, pat)
$ j = 2
$fill:
$ WRITE f F$FAO("K00000!UL0", j) + F$EXTRACT(0, 30, pat)
$ j = j + 1
$ IF j .LE. 9 THEN GOTO fill
$ k = "K0000015"
$ IF m .EQ. 1 THEN k = "K0000095"
$ WRITE f k + F$EXTRACT(0, t - base, pat)
$ CLOSE f
$ ANALYZE/RMS_FILE/FDL/OUTPUT=P.ANL P.DAT
$ WRITE SYS$OUTPUT "@@ ", fdl, " ", m, " ", t
$ SEARCH P.ANL "DATA_SPACE_OCCUPIED"
$ DELETE P.DAT;*,P.ANL;*
$ t = t + 1
$ IF t .LE. 510 THEN GOTO ts
$ m = m + 1
$ IF m .LE. 1 THEN GOTO modes
$ i = i + 1
$ GOTO fdls
$done:
$ EXIT 1
