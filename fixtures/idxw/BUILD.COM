$ SET NOON
$ say = "WRITE SYS$OUTPUT"
$ vs = F$FAO("!40*v")
$ dash = F$FAO("!56*-")
$!
$! SA0..SA7, SD0..SD7: a full bucket of 7 records (put ascending, or
$! descending), then an 8th that goes at position p
$ p = 0
$sp:
$ CREATE/FDL=SP.FDL SA'p'.DAT
$ OPEN/READ/WRITE f SA'p'.DAT
$ i = 1
$spa:
$ WRITE f F$FAO("K!2ZL0", i) + dash
$ i = i + 1
$ IF i .LE. 7 THEN GOTO spa
$ WRITE f F$FAO("K!2ZL5", p) + dash
$ CLOSE f
$ CREATE/FDL=SP.FDL SD'p'.DAT
$ OPEN/READ/WRITE f SD'p'.DAT
$ i = 7
$spd:
$ WRITE f F$FAO("K!2ZL0", i) + dash
$ i = i - 1
$ IF i .GE. 1 THEN GOTO spd
$ WRITE f F$FAO("K!2ZL5", p) + dash
$ CLOSE f
$ p = p + 1
$ IF p .LE. 7 THEN GOTO sp
$!
$! SV1, SV2: records of different sizes, split by bytes or by count
$ CREATE/FDL=SV.FDL SV1.DAT
$ OPEN/READ/WRITE f SV1.DAT
$ WRITE f "K010" + F$FAO("!196*a")
$ i = 2
$sv1:
$ WRITE f F$FAO("K!2ZL0", i) + F$FAO("!26*b")
$ i = i + 1
$ IF i .LE. 7 THEN GOTO sv1
$ WRITE f "K045" + F$FAO("!46*c")
$ CLOSE f
$ CREATE/FDL=SV.FDL SV2.DAT
$ OPEN/READ/WRITE f SV2.DAT
$ i = 1
$sv2:
$ WRITE f F$FAO("K!2ZL0", i) + F$FAO("!26*b")
$ i = i + 1
$ IF i .LE. 6 THEN GOTO sv2
$ WRITE f "K070" + F$FAO("!196*a")
$ WRITE f "K035" + F$FAO("!46*c")
$ CLOSE f
$!
$! CR: when record compression takes a run of a character
$ CREATE/FDL=CR.FDL CR.DAT
$ OPEN/READ/WRITE f CR.DAT
$ i = 0
$ l = 2
$cr1:
$ i = i + 1
$ WRITE f F$FAO("K!3ZL", i) + F$EXTRACT(0, l, vs)
$ l = l + 1
$ IF l .LE. 9 THEN GOTO cr1
$ q = 1
$cr2:
$ l = 3
$cr2l:
$ i = i + 1
$ WRITE f F$FAO("K!3ZL", i) + F$EXTRACT(0, q, "abcdefg") + F$EXTRACT(0, l, vs)
$ l = l + 1
$ IF l .LE. 9 THEN GOTO cr2l
$ q = q + 1
$ IF q .LE. 7 THEN GOTO cr2
$ q = 0
$cr3:
$ l = 3
$cr3l:
$ i = i + 1
$ WRITE f F$FAO("K!3ZL", i) + F$EXTRACT(0, q, "abc") + F$EXTRACT(0, l, vs) + "xyz"
$ i = i + 1
$ WRITE f F$FAO("K!3ZL", i) + "zzzzzzzzz" + F$EXTRACT(0, q, "abc") + F$EXTRACT(0, l, vs)
$ l = l + 1
$ IF l .LE. 8 THEN GOTO cr3l
$ q = q + 1
$ IF q .LE. 3 THEN GOTO cr3
$ CLOSE f
$!
$! NK, FK: no compression, the key away from the front
$ CREATE/FDL=NK.FDL NK.DAT
$ OPEN/READ/WRITE f NK.DAT
$ n = 1
$nk:
$ r = n * 7 - (n * 7 / 23) * 23
$ WRITE f "hdr00" + F$FAO("K!3ZL", r) + "---" + F$FAO("C!3ZL", r - (r / 3) * 3) + F$EXTRACT(0, r, vs)
$ n = n + 1
$ IF n .LE. 22 THEN GOTO nk
$ say "@@ short"
$ WRITE f "hdr"
$ say "@@ long"
$ WRITE f "hdr00K999---C000" + vs + vs
$ CLOSE f
$ CREATE/FDL=FK.FDL FK.DAT
$ OPEN/READ/WRITE f FK.DAT
$ n = 1
$fk:
$ r = n * 7 - (n * 7 / 23) * 23
$ WRITE f "hdr00" + F$FAO("K!3ZL", r) + "-----------"
$ n = n + 1
$ IF n .LE. 22 THEN GOTO fk
$ CLOSE f
$!
$! SC: one alternate key value for all records (a SIDR over buckets),
$! and a key with no duplicates and no changes
$ CREATE/FDL=SC.FDL SC.DAT
$ OPEN/READ/WRITE f SC.DAT
$ n = 1
$sc:
$ r = n * 7 - (n * 7 / 151) * 151
$ WRITE f F$FAO("R!4ZL-SAME-U!4ZL----", r, r)
$ n = n + 1
$ IF n .LE. 150 THEN GOTO sc
$ READ/DELETE/KEY="R0007" f rec
$ READ/DELETE/KEY="R0100" f rec
$ say "@@ dup"
$ WRITE f "R9999-SAME-U0005----"
$ say "@@ chg"
$ READ/KEY="R0005" f rec
$ WRITE/UPDATE f "R0005-SAME-U9999----"
$ say "@@ chg0"
$ READ/KEY="R0006" f rec
$ WRITE/UPDATE f "R9998-SAME-U0006----"
$ say "@@ done"
$ CLOSE f
$!
$! DM1..DM5: deletes of moved records and of a bucket's last record
$ d = 1
$dm:
$ CREATE/FDL=SP.FDL DM'd'.DAT
$ OPEN/READ/WRITE f DM'd'.DAT
$ i = 1
$dma:
$ WRITE f F$FAO("K!2ZL0", i) + dash
$ i = i + 1
$ IF i .LE. 7 THEN GOTO dma
$ IF d .LE. 2 THEN WRITE f "K035" + dash
$ IF d .EQ. 1 THEN READ/DELETE/KEY="K060" f rec
$ IF d .EQ. 2 THEN READ/DELETE/KEY="K070" f rec
$ IF d .EQ. 3 THEN READ/DELETE/KEY="K070" f rec
$ IF d .EQ. 3 THEN WRITE f "K070" + dash
$ IF d .EQ. 4 THEN READ/DELETE/KEY="K070" f rec
$ IF d .EQ. 4 THEN WRITE f "K080" + dash
$ IF d .NE. 5 THEN GOTO dmx
$ i = 1
$dmd:
$ kk = F$FAO("K!2ZL0", i)
$ READ/DELETE/KEY='kk' f rec
$ i = i + 1
$ IF i .LE. 7 THEN GOTO dmd
$ WRITE f "K040" + dash
$dmx:
$ CLOSE f
$ d = d + 1
$ IF d .LE. 5 THEN GOTO dm
$!
$! UP: updates that grow a record past its bucket, and shrink one
$ CREATE/FDL=SV.FDL UP.DAT
$ OPEN/READ/WRITE f UP.DAT
$ i = 1
$up:
$ WRITE f F$FAO("K!2ZL0", i) + F$FAO("!56*b")
$ i = i + 1
$ IF i .LE. 7 THEN GOTO up
$ READ/KEY="K030" f rec
$ WRITE/UPDATE f rec + F$FAO("!200*g")
$ READ/KEY="K050" f rec
$ WRITE/UPDATE f "K050short"
$ CLOSE f
$!
$! MC: a deep index with compressed keys, and an overlapping second key
$ CREATE/FDL=MC.FDL MC.DAT
$ OPEN/READ/WRITE f MC.DAT
$ n = 1
$mc:
$ k = n * 37 - (n * 37 / 307) * 307
$ s = F$FAO("!5ZL", k)
$ WRITE f s + s + s + s + s + s + s + s + F$FAO("!20AS", F$ELEMENT(k - (k / 4) * 4, ",", "alpha,beta,gamma,delta"))
$ n = n + 1
$ IF n .LE. 300 THEN GOTO mc
$ CLOSE f
$!
$ i = 0
$dump:
$ e = F$ELEMENT(i, ",", "SC/2,MC/1,NK/1,FK/0,CR/0,UP/0,DM1/0,DM2/0,DM3/0,DM4/0,DM5/0")
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
$ f = F$ELEMENT(i, ",", "SA0,SA1,SA2,SA3,SA4,SA5,SA6,SA7,SD0,SD1,SD2,SD3,SD4,SD5,SD6,SD7,SV1,SV2,CR,NK,FK,SC,DM1,DM2,DM3,DM4,DM5,UP,MC")
$ IF f .EQS. "," THEN GOTO done
$ ANALYZE/RMS_FILE/FDL/OUTPUT='f'.ANL 'f'.DAT
$ ANALYZE/RMS_FILE/CHECK/OUTPUT='f'.CHK 'f'.DAT
$ i = i + 1
$ GOTO anaf
$done:
$ EXIT 1
