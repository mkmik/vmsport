$! Sequential files where records meet block boundaries: BLOCK_SPAN no
$! (VAR, FIX, VFC) and records longer than a block (VAR), plus a FORTRAN
$! carriage control file. The .DAT files are the fixtures.
$ SET NOON
$ CREATE/FDL=VARBLK.FDL VARBLK.DAT
$ OPEN/APPEND out VARBLK.DAT
$ n = 1
$varblk:
$ WRITE out F$FAO("!2ZL!48*-", n)
$ n = n + 1
$ IF n .LE. 25 THEN GOTO varblk
$! One DCL element holds at most 255 characters: build longer records
$! from symbols and write them with /SYMBOL.
$ v = F$FAO("!250*v")
$ v = v + v + F$FAO("!9*v")
$ WRITE/SYMBOL out v
$ w = F$FAO("!255*w")
$ w = w + w
$ WRITE/SYMBOL out w
$ WRITE out "after"
$ CLOSE out
$!
$ CREATE/FDL=FIXBLK.FDL FIXBLK.DAT
$ OPEN/APPEND out FIXBLK.DAT
$ n = 1
$fixblk:
$ WRITE out F$FAO("!3ZL!98*=", n)
$ n = n + 1
$ IF n .LE. 12 THEN GOTO fixblk
$ CLOSE out
$!
$ CREATE/FDL=VFCBLK.FDL VFCBLK.DAT
$ OPEN/APPEND out VFCBLK.DAT
$ n = 1
$vfcblk:
$ WRITE out F$FAO("!2ZL!59*~", n)
$ n = n + 1
$ IF n .LE. 20 THEN GOTO vfcblk
$ CLOSE out
$!
$ CREATE/FDL=VARLONG.FDL VARLONG.DAT
$ OPEN/APPEND out VARLONG.DAT
$ a = F$FAO("!250*a")
$ a = a + a + F$FAO("!200*a")
$ WRITE/SYMBOL out a
$ b = F$FAO("!150*b")
$ b = b + b
$ WRITE/SYMBOL out b
$ c = F$FAO("!250*c")
$ c = c + c + c + c + F$FAO("!100*c")
$ WRITE/SYMBOL out c
$ WRITE out "end"
$ CLOSE out
$!
$ CREATE/FDL=FTN.FDL FTN.DAT
$ OPEN/APPEND out FTN.DAT
$ WRITE out " single"
$ WRITE out "0double"
$ WRITE out "1page"
$ WRITE out "+over"
$ WRITE out "$prompt"
$ WRITE out "xother"
$ WRITE out ""
$ CLOSE out
$ TYPE FTN.DAT
$ DIRECTORY/FULL *.DAT
$ i = 0
$ana:
$ f = F$ELEMENT(i, ",", "VARBLK,FIXBLK,VFCBLK,VARLONG,FTN")
$ IF f .EQS. "," THEN GOTO done
$ ANALYZE/RMS_FILE/CHECK 'f'.DAT
$ i = i + 1
$ GOTO ana
$done:
$ EXIT 1
