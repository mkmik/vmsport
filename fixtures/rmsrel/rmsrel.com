$! Relative files of several shapes, made from the FDL files here: records
$! written in order, some deleted and updated, appends after them, a file
$! that has to extend, one left empty. The .DAT files are the fixtures.
$ SET NOON
$ WRITE SYS$OUTPUT "@@ messages"
$ WRITE SYS$OUTPUT F$MESSAGE(98994)
$ WRITE SYS$OUTPUT F$MESSAGE(98978)
$ WRITE SYS$OUTPUT F$MESSAGE(99732)
$ WRITE SYS$OUTPUT F$MESSAGE(99788)
$ WRITE SYS$OUTPUT F$MESSAGE(100004)
$ WRITE SYS$OUTPUT F$MESSAGE(98938)
$ WRITE SYS$OUTPUT "@@ rzero"
$ CREATE/FDL=RZERO.FDL RZERO.DAT
$ WRITE SYS$OUTPUT F$FAO("create !XL", $STATUS)
$!
$ WRITE SYS$OUTPUT "@@ rfix"
$ CREATE/FDL=RFIX.FDL RFIX.DAT
$ OPEN/APPEND f RFIX.DAT
$ n = 1
$fixw:
$ WRITE f F$FAO("fixrec !3ZL", n)
$ n = n + 1
$ IF n .LE. 25 THEN GOTO fixw
$ CLOSE f
$ OPEN/READ/WRITE f RFIX.DAT
$ i = 0
$fixd:
$ i = i + 1
$ IF i .EQ. 3 .OR. i .EQ. 4 .OR. i .EQ. 10
$ THEN
$   READ/DELETE/END_OF_FILE=fixdd f r
$ ELSE
$   READ/END_OF_FILE=fixdd f r
$   IF i .EQ. 5 THEN WRITE/UPDATE f "updated 05"
$ ENDIF
$ IF i .LT. 12 THEN GOTO fixd
$fixdd:
$ CLOSE f
$ OPEN/APPEND f RFIX.DAT
$ n = 26
$fixm:
$ WRITE f F$FAO("fixrec !3ZL", n)
$ s = $STATUS
$ WRITE SYS$OUTPUT F$FAO("put !UL !XL", n, s)
$ n = n + 1
$ IF n .LE. 32 THEN GOTO fixm
$ CLOSE f
$ file = "RFIX.DAT"
$ GOSUB show
$!
$ WRITE SYS$OUTPUT "@@ rvfc"
$ CREATE/FDL=RVFC.FDL RVFC.DAT
$ OPEN/APPEND f RVFC.DAT
$ fill = "abcdefghijklmnopqrstuvwxyz0123456789"
$ n = 1
$vfcw:
$ k = n * 7 - (n * 7 / 29) * 29
$ WRITE f F$FAO("!2ZL", n) + F$EXTRACT(0, k, fill)
$ n = n + 1
$ IF n .LE. 12 THEN GOTO vfcw
$ CLOSE f
$ OPEN/READ/WRITE f RVFC.DAT
$ i = 0
$vfcd:
$ i = i + 1
$ IF i .EQ. 4 .OR. i .EQ. 12
$ THEN
$   READ/DELETE/END_OF_FILE=vfcdd f r
$ ELSE
$   READ/END_OF_FILE=vfcdd f r
$   IF i .EQ. 6 THEN WRITE/UPDATE f "short"
$ ENDIF
$ GOTO vfcd
$vfcdd:
$ CLOSE f
$ OPEN/APPEND f RVFC.DAT
$ WRITE f "appended"
$ CLOSE f
$ file = "RVFC.DAT"
$ GOSUB show
$!
$ WRITE SYS$OUTPUT "@@ rvar"
$ CREATE/FDL=RVAR.FDL RVAR.DAT
$ OPEN/APPEND f RVAR.DAT
$ long = F$FAO("!600*v")
$ n = 1
$varw:
$ WRITE f F$FAO("!2ZL", n) + F$EXTRACT(0, n * 29, long)
$ n = n + 1
$ IF n .LE. 20 THEN GOTO varw
$ CLOSE f
$ file = "RVAR.DAT"
$ GOSUB show
$!
$ WRITE SYS$OUTPUT "@@ rempty"
$ CREATE/FDL=REMPTY.FDL REMPTY.DAT
$ file = "REMPTY.DAT"
$ GOSUB show
$!
$ WRITE SYS$OUTPUT "@@ directory"
$ DIRECTORY/FULL R*.DAT
$ i = 0
$ana:
$ f = F$ELEMENT(i, ",", "RFIX,RVFC,RVAR,REMPTY")
$ IF f .EQS. "," THEN GOTO done
$ ANALYZE/RMS_FILE/FDL/OUTPUT='f'.ANL 'f'.DAT
$ ANALYZE/RMS_FILE/CHECK/OUTPUT='f'.CHK 'f'.DAT
$ i = i + 1
$ GOTO ana
$done:
$ EXIT 1
$!
$show:
$ OPEN/READ in 'file'
$shownext:
$ READ/END_OF_FILE=showend in r
$ WRITE SYS$OUTPUT "[", r, "]"
$ GOTO shownext
$showend:
$ CLOSE in
$ RETURN
