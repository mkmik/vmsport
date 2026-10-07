/* lockdemo: the lock manager from C. A parent holds a resource in EX; its
 * child can't have it without waiting (LCK$M_NOQUEUE), waits for it, and
 * gets it when the parent converts down to NL. */
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <sys/wait.h>
#include <descrip.h>
#include <lckdef.h>
#include <lksbdef.h>
#include <ssdef.h>
#include <starlet.h>

static $DESCRIPTOR(resource, "VPT_DEMO_RESOURCE");

int main(void) {
    struct lksb parent, child;
    int fds[2];
    char c;
    unsigned int st, cvt;
    int status;

    st = sys$enqw(0, LCK$K_EXMODE, &parent, 0, &resource);
    printf("parent EX: %08X lksb %04X\n", st, parent.lksb$w_status);
    st = sys$enqw(0, LCK$K_EXMODE, &child, LCK$M_NOQUEUE, &resource);
    printf("parent EX again, NOQUEUE: %08X (SS$_NOTQUEUED %08X)\n", st, SS$_NOTQUEUED);
    fflush(stdout);
    if (pipe(fds) != 0)
        return 1;
    if (fork() == 0) {
        st = sys$enqw(0, LCK$K_EXMODE, &child, LCK$M_NOQUEUE, &resource);
        printf("child EX, NOQUEUE: %08X\n", st);
        fflush(stdout);
        if (write(fds[1], "x", 1) != 1)
            _exit(1);
        st = sys$enqw(0, LCK$K_EXMODE, &child, 0, &resource);
        printf("child EX, waited: %08X lksb %04X\n", st, child.lksb$w_status);
        fflush(stdout);
        _exit(0);
    }
    if (read(fds[0], &c, 1) != 1)
        return 1;
    usleep(200000);
    cvt = sys$enqw(0, LCK$K_NLMODE, &parent, LCK$M_CONVERT);
    wait(&status);
    printf("parent converted to NL: %08X; child exited %d\n", cvt, WEXITSTATUS(status));
    /* The child is gone, and its lock with it. */
    st = sys$enqw(0, LCK$K_EXMODE, &parent, LCK$M_CONVERT | LCK$M_NOQUEUE | LCK$M_SYNCSTS);
    printf("parent back to EX, NOQUEUE SYNCSTS: %08X\n", st);
    st = sys$enqw(0, LCK$K_PRMODE, &parent, LCK$M_VALBLK, &resource);
    printf("value block: %08X\n", st);
    printf("deq: %08X\n", sys$deq(parent.lksb$l_lkid));
    printf("deq again: %08X\n", sys$deq(parent.lksb$l_lkid));
    printf("deq all: %08X\n", sys$deq(0, 0, 0, LCK$M_DEQALL));
    return 0;
}
