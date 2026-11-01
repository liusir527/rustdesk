import 'package:flutter/material.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/common/widgets/login.dart';
import 'package:flutter_hbb/consts.dart';
import 'package:flutter_hbb/desktop/pages/desktop_setting_page.dart';
import 'package:flutter_hbb/desktop/widgets/tabbar_widget.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:flutter_hbb/models/peer_tab_model.dart';
import 'package:get/get.dart';
import 'package:provider/provider.dart';

const double _kSidebarWidth = 178.0;
const Color _kSidebarBg = Color(0xFFF0F3F8);
const Color _kSidebarDarkBg = Color(0xFF1F2027);
const Color _kSidebarSelectedBg = Color(0xFFE7EBF0);
const Color _kSidebarHoverBg = Color(0xFFEAEEF4);
const Color _kSidebarText = Color(0xFF4F5665);
const Color _kSidebarIcon = Color(0xFF8A919F);
const Color _kSidebarAccent = Color(0xFF0175FF);

/// Window level navigation rail, styled after the ToDesk desktop client.
class DesktopSidebar extends StatelessWidget {
  const DesktopSidebar({Key? key, required this.controller}) : super(key: key);

  final DesktopTabController controller;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: _kSidebarWidth,
      color: Theme.of(context).brightness == Brightness.dark
          ? _kSidebarDarkBg
          : _kSidebarBg,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const _SidebarDragArea(),
          const _AccountHeader(),
          Expanded(
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: _buildNavItems(context),
              ),
            ),
          ),
          const _SidebarFooter(),
        ],
      ),
    );
  }

  List<Widget> _buildNavItems(BuildContext context) {
    return [
      Consumer<PeerTabModel>(
        builder: (context, peerTab, child) => Obx(() {
          final selectedKey = controller.state.value.selectedTabInfo.key;
          final onHome = selectedKey == kTabLabelHomePage;
          final currentPeerTab = peerTab.currentTab;
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final index in peerTab.visibleEnabledOrderedIndexs)
                _NavItem(
                  icon: _outlinedTabIcon(index),
                  selectedIcon: peerTab.tabIcon(index),
                  label: peerTab.tabTooltip(index),
                  selected: onHome && currentPeerTab == index,
                  onTap: () {
                    _selectHomeTab();
                    peerTab.setCurrentTab(index);
                  },
                ),
            ],
          );
        }),
      ),
      const Padding(
        padding: EdgeInsets.symmetric(horizontal: 22, vertical: 6),
        child: Divider(height: 1),
      ),
      Obx(() => _NavItem(
            icon: Icons.settings_outlined,
            selectedIcon: Icons.settings,
            label: translate('Settings'),
            selected: controller.state.value.selectedTabInfo.key ==
                kTabLabelSettingPage,
            onTap: () {
              if (!bind.isDisableSettings()) {
                DesktopSettingPage.switch2page(SettingsTabKey.general);
              }
            },
          )),
      _NavItem(
        icon: Icons.info_outline,
        selectedIcon: Icons.info,
        label: translate('About'),
        selected: false,
        onTap: () {
          if (!bind.isDisableSettings()) {
            DesktopSettingPage.switch2page(SettingsTabKey.about);
          }
        },
      ),
    ];
  }

  void _selectHomeTab() {
    controller.jumpToByKey(kTabLabelHomePage);
  }

  /// Outlined counterpart of [PeerTabModel.tabIcon] for the unselected state.
  static IconData _outlinedTabIcon(int index) {
    switch (PeerTabIndex.values[index]) {
      case PeerTabIndex.recent:
        return Icons.access_time_outlined;
      case PeerTabIndex.fav:
        return Icons.star_border;
      case PeerTabIndex.lan:
        return Icons.explore_outlined;
      case PeerTabIndex.ab:
        return IconFont.addressBook;
      case PeerTabIndex.group:
        return IconFont.deviceGroupOutline;
    }
  }
}

/// Height of the window control row at the top of the sidebar.
///
/// The tab bar used to reserve this space for the macOS traffic lights; the
/// sidebar takes it over as a drag handle once it is the leading widget.
const double _kSidebarDragAreaHeight = 28.0;

class _SidebarDragArea extends StatelessWidget {
  const _SidebarDragArea({Key? key}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      behavior: HitTestBehavior.translucent,
      onPanStart: (_) => startDragging(true),
      onPanCancel: () {
        if (isMacOS) {
          setMovable(true, false);
        }
      },
      onPanEnd: (_) {
        if (isMacOS) {
          setMovable(true, false);
        }
      },
      child: const SizedBox(
          height: _kSidebarDragAreaHeight, width: double.infinity),
    );
  }
}

class _NavItem extends StatefulWidget {
  const _NavItem({
    Key? key,
    required this.icon,
    required this.selectedIcon,
    required this.label,
    required this.selected,
    required this.onTap,
  }) : super(key: key);

  final IconData icon;
  final IconData selectedIcon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  State<_NavItem> createState() => _NavItemState();
}

class _NavItemState extends State<_NavItem> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    final selected = widget.selected;
    final Color bg = selected
        ? (dark ? Colors.white10 : _kSidebarSelectedBg)
        : (_hover ? (dark ? Colors.white10 : _kSidebarHoverBg) : Colors.transparent);
    final Color fg = selected
        ? _kSidebarAccent
        : (dark ? Colors.white70 : _kSidebarText);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 2),
      child: Material(
        color: bg,
        borderRadius: BorderRadius.circular(8),
        child: InkWell(
          borderRadius: BorderRadius.circular(8),
          onHover: (value) => setState(() => _hover = value),
          onTap: widget.onTap,
          child: SizedBox(
            height: 34,
            child: Row(
              children: [
                Icon(
                  selected ? widget.selectedIcon : widget.icon,
                  size: 18,
                  color: selected ? _kSidebarAccent : (dark ? Colors.white54 : _kSidebarIcon),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    widget.label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 13,
                      color: fg,
                      fontWeight:
                          selected ? FontWeight.w600 : FontWeight.w400,
                    ),
                  ),
                ),
              ],
            ),
          ).paddingSymmetric(horizontal: 10),
        ),
      ),
    );
  }
}

class _AccountHeader extends StatelessWidget {
  const _AccountHeader({Key? key}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return Padding(
      padding: const EdgeInsets.only(top: 10, bottom: 10),
      child: Column(
        children: [
          Obx(() {
            final avatar = bind.mainResolveAvatarUrl(
                avatar: gFFI.userModel.avatar.value);
            final fallback = CircleAvatar(
              radius: 20,
              backgroundColor:
                  dark ? Colors.white12 : const Color(0xFFDDE3EC),
              child: Icon(Icons.person,
                  size: 22, color: dark ? Colors.white54 : _kSidebarIcon),
            );
            final image = buildAvatarWidget(
                avatar: avatar, size: 40, fallback: fallback);
            return InkWell(
              borderRadius: BorderRadius.circular(20),
              onTap: () => _onTapAccount(context),
              child: SizedBox(width: 40, height: 40, child: image),
            );
          }),
          const SizedBox(height: 8),
          Obx(() {
            final name = gFFI.userModel.displayNameOrUserName;
            final label = name.isEmpty ? translate('Login') : name;
            return InkWell(
              onTap: () => _onTapAccount(context),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 12),
                child: Text(
                  label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 14,
                    fontWeight: FontWeight.w500,
                    color: dark ? Colors.white : const Color(0xFF2B2F38),
                  ),
                ),
              ),
            );
          }),
        ],
      ),
    );
  }

  void _onTapAccount(BuildContext context) {
    if (bind.isDisableAccount()) return;
    if (gFFI.userModel.isLogin) {
      DesktopSettingPage.switch2page(SettingsTabKey.account);
    } else {
      loginDialog();
    }
  }
}

class _SidebarFooter extends StatelessWidget {
  const _SidebarFooter({Key? key}) : super(key: key);

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return Padding(
      padding: const EdgeInsets.only(left: 14, right: 14, bottom: 12),
      child: Row(
        children: [
          loadIcon(18),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              bind.mainGetAppNameSync(),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontSize: 12,
                color: dark ? Colors.white38 : _kSidebarIcon,
              ),
            ),
          ),
          FutureBuilder<String>(
            future: bind.mainGetVersion(),
            builder: (context, snapshot) {
              final version = snapshot.data;
              if (version == null || version.isEmpty) {
                return const SizedBox.shrink();
              }
              return Text(
                version,
                style: TextStyle(
                  fontSize: 11,
                  color: dark ? Colors.white30 : _kSidebarIcon,
                ),
              );
            },
          ),
        ],
      ),
    );
  }
}
