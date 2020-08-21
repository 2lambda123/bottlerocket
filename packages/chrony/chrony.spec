Name: %{_cross_os}chrony
Version: 3.5.1
Release: 1%{?dist}
Summary: A versatile implementation of the Network Time Protocol
License: GPL-2.0-only
URL: https://chrony.tuxfamily.org
Source0: https://download.tuxfamily.org/chrony/chrony-%{version}.tar.gz
Source1: chronyd.service
Source2: chrony-conf
Source3: chrony-sysusers.conf
Source4: chrony-tmpfiles.conf
BuildRequires: %{_cross_os}glibc-devel
BuildRequires: %{_cross_os}libcap-devel
BuildRequires: %{_cross_os}libseccomp-devel
BuildRequires: %{_cross_os}ncurses-devel
BuildRequires: %{_cross_os}readline-devel
Requires: %{_cross_os}libcap
Requires: %{_cross_os}libseccomp

# Patches taken from upstream

# Update seccomp filter to work with glibc 2.31
# Reworked version of
# https://git.tuxfamily.org/chrony/chrony.git/patch/sys_linux.c?id=02ada36838e48942dd1ecd0513c3449fcf9135df
Patch0: 0001-sys_linux-add-support-for-TCP-sockets.patch
# https://git.tuxfamily.org/chrony/chrony.git/patch/sys_linux.c?id=429c4468b0058d9c2e2fffbf6660b0f1581af6af
Patch1: 0002-sys_linux-allow-F_GETFL-in-seccomp-filter.patch
# https://git.tuxfamily.org/chrony/chrony.git/patch/sys_linux.c?id=0cf506c92967c84f9ed83ba9e1be946a7fda6425
Patch2: 0003-sys_linux-allow-clock_adjtime-in-seccomp-filter.patch
# https://git.tuxfamily.org/chrony/chrony.git/patch/sys_linux.c?id=994409a03697b8df68115342dc8d1e7ceeeb40bd
Patch3: 0004-sys_linux-allow-renameat2-in-seccomp-filter.patch

%description
%{summary}.

%package tools
Summary: Command-line interface for chrony daemon
Requires: %{_cross_os}chrony
Requires: %{_cross_os}readline
Requires: %{_cross_os}ncurses

%description tools
%{summary}.

%prep
%autosetup -n chrony-%{version} -p1

%build
# chrony uses a custom hand-rolled configure script
%set_cross_build_flags \
CC=%{_cross_target}-gcc \
./configure \
 --prefix="%{_cross_prefix}" \
 --enable-scfilter

%make_build

%install
%make_install

install -d %{buildroot}%{_cross_unitdir}
install -p -m 0644 %{S:1} %{buildroot}%{_cross_unitdir}/chronyd.service
install -d %{buildroot}%{_cross_templatedir}
install -p -m 0644 %{S:2} %{buildroot}%{_cross_templatedir}/chrony-conf
install -d %{buildroot}%{_cross_sysusersdir}
install -p -m 0644 %{S:3} %{buildroot}%{_cross_sysusersdir}/chrony.conf
install -d %{buildroot}%{_cross_tmpfilesdir}
install -p -m 0644 %{S:4} %{buildroot}%{_cross_tmpfilesdir}/chrony.conf

%files
%license COPYING
%{_cross_attribution_file}
%dir %{_cross_templatedir}
%{_cross_sbindir}/chronyd
%{_cross_templatedir}/chrony-conf
%{_cross_unitdir}/chronyd.service
%{_cross_sysusersdir}/chrony.conf
%{_cross_tmpfilesdir}/chrony.conf
%exclude %{_cross_mandir}

%files tools
%{_cross_bindir}/chronyc

%changelog
