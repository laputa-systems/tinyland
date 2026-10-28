project('seatd-client-oracle', 'c', version: '0.9.3', license: 'MIT',
  meson_version: '>=0.60.0', default_options: ['c_std=c11', 'warning_level=3', 'werror=true'])

add_project_arguments('-D_XOPEN_SOURCE=700', '-DLIBSEAT=1', '-DSEATD_ENABLED=1',
  '-DSEATD_DEFAULTPATH="/run/seatd.sock"', language: 'c')
headers = include_directories('include')
private = static_library('seat-private',
  ['common/connection.c', 'common/linked_list.c', 'common/log.c', 'libseat/backend/seatd.c'],
  include_directories: headers)
lib = shared_library('seat', ['libseat/libseat.c', 'libseat/backend/noop.c'],
  soversion: '1', link_with: private, include_directories: headers,
  link_args: '-Wl,--version-script=' + meson.current_source_dir() / 'libseat/libseat.syms',
  link_depends: 'libseat/libseat.syms')
executable('simpletest', 'examples/simpletest/main.c', link_with: lib, include_directories: headers)
foreach name : ['connection', 'linked_list']
  test(name, executable(name + '_test', ['tests/' + name + '.c', 'common/' + name + '.c'],
    include_directories: headers))
endforeach
