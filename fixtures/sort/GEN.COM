$! P2: P1 records "nnnnn kk xx...", kk = nnnnn mod 7 (many ties), P3 x's.
$ OPEN/WRITE F 'P2'
$ i = 0
$ loop:
$ IF i .GE. 'P1' THEN GOTO done
$ WRITE F F$FAO("!5ZL !2ZL ", i, i - (i/7)*7), F$EXTRACT(0, 'P3', "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
$ i = i + 1
$ GOTO loop
$ done:
$ CLOSE F
