$! Makes an RMS file of every organization and record format from the FDL
$! files here, fills it, and analyzes it. The .DAT files are the fixtures.
$ SET NOON
$ lines = "first|second, a bit longer||odd|x|last line of the file"
$ FOR_SEQ = "SEQVAR,SEQVFC,SEQSTM,SEQLFSTM,SEQCRSTM"
$ i = 0
$seq:
$ f = F$ELEMENT(i, ",", FOR_SEQ)
$ IF f .EQS. "," THEN GOTO fix
$ CREATE/FDL='f'.FDL 'f'.DAT
$ OPEN/APPEND out 'f'.DAT
$ j = 0
$seqrec:
$ r = F$ELEMENT(j, "|", lines)
$ IF r .EQS. "|" THEN GOTO seqdone
$ WRITE out r
$ j = j + 1
$ GOTO seqrec
$seqdone:
$ CLOSE out
$ i = i + 1
$ GOTO seq
$fix:
$ CREATE/FDL=SEQFIX.FDL SEQFIX.DAT
$ OPEN/APPEND out SEQFIX.DAT
$ WRITE out "thirteen char"
$ WRITE out "0123456789abc"
$ WRITE out "             "
$ CLOSE out
$!
$ CREATE/FDL=REL.FDL REL.DAT
$ OPEN/APPEND out REL.DAT
$ dots = F$FAO("!40*.")
$ n = 1
$rel:
$ WRITE out F$FAO("relative record !UL", n) + F$EXTRACT(0, n, dots)
$ n = n + 1
$ IF n .LE. 20 THEN GOTO rel
$ CLOSE out
$!
$ CREATE/FDL=IDX.FDL IDX.DAT
$ OPEN/READ/WRITE out IDX.DAT
$ cities = "ROME,PARIS,ZAGREB,OSLO,LIMA,TOKYO,QUITO"
$ n = 1
$idx:
$ k = n * 37 - (n * 37 / 151) * 151
$ id = F$FAO("ID!6ZL", k)
$ city = F$ELEMENT(k - (k / 7) * 7, ",", cities)
$ a = F$FAO("!4ZL", k / 10)
$ b = F$FAO("!4ZL", k - (k / 3) * 3)
$ num = F$FAO("N!3ZL", k / 2)
$ rec = F$FAO("!8AS!10AS!2*.!4AS!6*.!4AS!6*.!4AS!20*.", id, city, a, b, num)
$ WRITE out rec
$ n = n + 1
$ IF n .LE. 150 THEN GOTO idx
$ READ/DELETE/KEY="ID000010" out rec
$ READ/DELETE/KEY="ID000011" out rec
$ READ/DELETE/KEY="ID000100" out rec
$ READ/KEY="ID000020" out rec
$ rec = F$EXTRACT(0, 8, rec) + "NEWCITY   " + F$EXTRACT(18, 46, rec)
$ WRITE/UPDATE out rec
$ CLOSE out
$!
$ CREATE/FDL=IDXVAR.FDL IDXVAR.DAT
$ OPEN/READ/WRITE out IDXVAR.DAT
$ pad = F$FAO("!130*v")
$ n = 1
$idxvar:
$ k = n * 13 - (n * 13 / 61) * 61
$ WRITE out F$FAO("K!5ZL", k) + F$EXTRACT(0, k * 2, pad)
$ n = n + 1
$ IF n .LE. 60 THEN GOTO idxvar
$ CLOSE out
$!
$ DIRECTORY/FULL *.DAT
$ i = 0
$ana:
$ f = F$ELEMENT(i, ",", "SEQVAR,SEQVFC,SEQSTM,SEQLFSTM,SEQCRSTM,SEQFIX,REL,IDX,IDXVAR")
$ IF f .EQS. "," THEN GOTO typ
$ ANALYZE/RMS_FILE/FDL/OUTPUT='f'.ANL 'f'.DAT
$ ANALYZE/RMS_FILE/CHECK/OUTPUT='f'.CHK 'f'.DAT
$ i = i + 1
$ GOTO ana
$typ:
$ TYPE SEQVAR.DAT,SEQVFC.DAT,SEQSTM.DAT,SEQLFSTM.DAT,SEQCRSTM.DAT,SEQFIX.DAT
$ TYPE REL.DAT
$ TYPE IDX.DAT
$ EXIT 1
