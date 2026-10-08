$! Symbols, expressions and substitution.
$ SET NOON
$ a = 7
$ b = "7"
$ c := hello world
$ d := "Hello  World"
$ e = "x" + "y"
$ SHOW SYMBOL a
$ SHOW SYMBOL b
$ SHOW SYMBOL c
$ SHOW SYMBOL d
$ SHOW SYMBOL e
$ g == 1
$ SHOW SYMBOL/GLOBAL g
$ SHOW SYMBOL g
$ WRITE SYS$OUTPUT a + 3, " ", a * 3, " ", a / 2, " ", -a / 2, " ", a - 10
$ WRITE SYS$OUTPUT 1 + 2 * 3, " ", (1 + 2) * 3, " ", 7 .AND. 3, " ", 4 .OR. 1
$ WRITE SYS$OUTPUT .NOT. 0, " ", .NOT. 1, " ", 2147483647 + 1
$ WRITE SYS$OUTPUT "ab" + "cd", " ", "abcabc" - "b", " ", "3" + 4, " ", 4 + "3"
$ WRITE SYS$OUTPUT "abc" .EQS. "ABC", " ", "abc" .LTS. "abd", " ", 5 .GT. 3
$ WRITE SYS$OUTPUT "10" .LT. "9", " ", "10" .LTS. "9", " ", "YES" .EQ. 1
$ WRITE SYS$OUTPUT %X10, " ", %O17, " ", %D99
$ x = "a"
$ WRITE SYS$OUTPUT "x is 'x' and ''x' twice"
$ n = "x"
$ WRITE SYS$OUTPUT &n
$ s = "0123456789"
$ s[2,3] := "abc"
$ SHOW SYMBOL s
$ bits = 0
$ bits[0,1] = 1
$ bits[4,4] = 15
$ SHOW SYMBOL bits
$ t = "TRUE"
$ IF t THEN WRITE SYS$OUTPUT "TRUE string is false"
$ t = "YES"
$ IF t THEN WRITE SYS$OUTPUT "YES string is true"
$ t = "0"
$ IF .NOT. t THEN WRITE SYS$OUTPUT "0 is false"
$ DELETE/SYMBOL a
$ SHOW SYMBOL a
$ abbrev*iation = "sym"
$ WRITE SYS$OUTPUT abbrev, " ", abbrevi, " ", abbreviation
$ EXIT
