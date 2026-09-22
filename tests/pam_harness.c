// SPDX-License-Identifier: GPL-3.0-or-later
//
// Runs a PAM stack from a directory of its own, so the module can be tried
// against a development daemon without root and without touching /etc/pam.d:
//
//   pam_harness CONFDIR SERVICE USER
//
// Prints every message the stack sends, answers nothing, and exits 0 when
// the stack let the user in.

#include <security/pam_appl.h>

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int conversation(int n, const struct pam_message **msg, struct pam_response **resp, void *data)
{
    (void)data;
    *resp = calloc((size_t)n, sizeof(struct pam_response));
    for (int i = 0; i < n; ++i) {
        const char *kind = msg[i]->msg_style == PAM_TEXT_INFO ? "info"
            : msg[i]->msg_style == PAM_ERROR_MSG             ? "error"
                                                             : "prompt";
        printf("%s: %s\n", kind, msg[i]->msg);
        fflush(stdout);
        if (msg[i]->msg_style == PAM_PROMPT_ECHO_OFF || msg[i]->msg_style == PAM_PROMPT_ECHO_ON) {
            // Nobody is typing a password here.
            return PAM_CONV_ERR;
        }
    }
    return PAM_SUCCESS;
}

int main(int argc, char **argv)
{
    if (argc != 4) {
        fprintf(stderr, "usage: %s CONFDIR SERVICE USER\n", argv[0]);
        return 2;
    }
    const struct pam_conv conv = {conversation, NULL};
    pam_handle_t *pamh = NULL;
    int rc = pam_start_confdir(argv[2], argv[3], &conv, argv[1], &pamh);
    if (rc != PAM_SUCCESS) {
        fprintf(stderr, "pam_start: %s\n", pam_strerror(pamh, rc));
        return 2;
    }
    rc = pam_authenticate(pamh, 0);
    printf("result: %s\n", pam_strerror(pamh, rc));
    pam_end(pamh, rc);
    return rc == PAM_SUCCESS ? 0 : 1;
}
