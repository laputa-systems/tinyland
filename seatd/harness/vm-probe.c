#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <linux/kd.h>
#include <linux/vt.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/ioctl.h>
#include <unistd.h>
#include "libseat.h"

#define DRM_SET_MASTER 0x641e
#define DRM_DROP_MASTER 0x641f
static int active, enables, disables;
static void enable(struct libseat *s, void *data) { (void)s; (void)data; active = 1; enables++; }
static void disable(struct libseat *s, void *data) { (void)data; active = 0; disables++; if (libseat_disable_seat(s)) { perror("disable"); exit(1); } }
static void require(int ok, const char *what) { if (!ok) { perror(what); exit(1); } }
static void dispatch_until(struct libseat *s, int *value, int want) { for (int i = 0; i < 30 && *value < want; i++) require(libseat_dispatch(s, 1000) >= 0, "dispatch"); require(*value >= want, "dispatch timeout"); }
int main(void) {
 struct libseat_seat_listener listener = { .enable_seat = enable, .disable_seat = disable };
 int tty = open("/dev/tty0", O_RDWR | O_CLOEXEC), fd, input, gpu, input_id, gpu_id, mode, kb;
 struct vt_mode vt;
 require(tty >= 0, "tty0");
 struct libseat *s = libseat_open_seat(&listener, NULL); require(s != NULL, "open seat");
 dispatch_until(s, &enables, 1);
 if (getenv("VM_SHUTDOWN_PID")) {
  struct pollfd connection = { .fd = libseat_get_fd(s), .events = POLLIN };
  require(kill(atoi(getenv("VM_SHUTDOWN_PID")), SIGTERM) == 0, "signal daemon");
  for (int i = 0; i < 20 && !(connection.revents & (POLLHUP | POLLERR)); i++) {
   require(poll(&connection, 1, 1000) > 0, "daemon disconnect timeout");
   if (connection.revents & POLLIN) { char buffer[256]; if (read(connection.fd, buffer, sizeof buffer) == 0) break; }
  }
  require(connection.revents & (POLLHUP | POLLERR), "daemon disconnect");
  fd = open("/dev/tty1", O_RDWR | O_CLOEXEC); require(fd >= 0, "tty1 shutdown");
  require(ioctl(fd, VT_GETMODE, &vt) == 0 && vt.mode == VT_AUTO, "shutdown vt mode");
  require(ioctl(fd, KDGETMODE, &mode) == 0 && mode == KD_TEXT, "shutdown display");
  require(ioctl(fd, KDGKBMODE, &kb) == 0 && kb == K_UNICODE, "shutdown keyboard");
  puts("VM_SHUTDOWN_RESTORE_OK"); return 0;
 }
 gpu_id = libseat_open_device(s, "/dev/dri/card0", &gpu); require(gpu_id >= 0, "open drm");
 input_id = libseat_open_device(s, "/dev/input/event0", &input); require(input_id >= 0, "open input");
 fd = open("/dev/dri/card0", O_RDWR | O_CLOEXEC); require(fd >= 0, "open competing drm");
 require(ioctl(fd, DRM_SET_MASTER, 0) < 0, "initial drm exclusivity"); close(fd);
 require(ioctl(input, EVIOCGVERSION, &mode) == 0, "initial evdev");
 require(libseat_switch_session(s, 2) == 0, "switch vt2");
 dispatch_until(s, &disables, 1);
 errno = 0; require(ioctl(input, EVIOCGVERSION, &mode) < 0 && errno == ENODEV, "evdev revoked");
 fd = open("/dev/dri/card0", O_RDWR | O_CLOEXEC); require(fd >= 0, "open second drm");
 require(ioctl(fd, DRM_SET_MASTER, 0) == 0, "second drm master");
 require(ioctl(fd, DRM_DROP_MASTER, 0) == 0, "drop second drm master"); close(fd);
 puts("VM_RELEASE_REVOKE_HANDOFF_OK"); fflush(stdout);
 require(ioctl(tty, VT_ACTIVATE, 1) == 0, "activate vt1");
 dispatch_until(s, &enables, 2);
 fd = open("/dev/dri/card0", O_RDWR | O_CLOEXEC); require(fd >= 0, "open competing drm after acquire");
 require(ioctl(fd, DRM_SET_MASTER, 0) < 0, "regained drm exclusivity"); close(fd);
 require(libseat_close_device(s, input_id) == 0, "close revoked input"); close(input);
 input_id = libseat_open_device(s, "/dev/input/event0", &input); require(input_id >= 0, "reopen input");
 require(ioctl(input, EVIOCGVERSION, &mode) == 0, "reopened evdev");
 puts("VM_ACQUIRE_REOPEN_OK"); fflush(stdout);
 require(libseat_close_device(s, input_id) == 0, "close input"); close(input);
 require(libseat_close_device(s, gpu_id) == 0, "close drm"); close(gpu);
 require(libseat_close_seat(s) == 0, "close seat");
 fd = open("/dev/tty1", O_RDWR | O_CLOEXEC); require(fd >= 0, "tty1");
 require(ioctl(fd, VT_GETMODE, &vt) == 0 && vt.mode == VT_AUTO, "restore vt mode");
 require(ioctl(fd, KDGETMODE, &mode) == 0 && mode == KD_TEXT, "restore display");
 require(ioctl(fd, KDGKBMODE, &kb) == 0 && kb == K_UNICODE, "restore keyboard");
 puts("VM_RESTORE_OK"); return 0;
}
