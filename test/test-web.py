#!/usr/bin/env python3
"""Verify the deployed Astro frontend against the real API. Password is read from stdin.
Requires Python Playwright and Chromium. Invoked optionally by smoke.py --browser-test.
"""
import argparse
import re
from pathlib import Path
import sys
from playwright.sync_api import sync_playwright, expect

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--origin', required=True)
parser.add_argument('--screenshots', type=Path, default=Path('/tmp/chublog-web-screenshots'))
args = parser.parse_args()
password = sys.stdin.read().strip()
assert password, 'Provide the temporary test password on stdin'
args.screenshots.mkdir(parents=True, exist_ok=True)

with sync_playwright() as playwright:
    browser = playwright.chromium.launch(headless=True)
    context = browser.new_context(viewport={'width': 1440, 'height': 1000}, locale='zh-CN')
    page = context.new_page()
    failures = []
    page.on('pageerror', lambda error: failures.append(str(error)))
    page.on('console', lambda message: failures.append(message.text) if 'Content Security Policy' in message.text or 'Refused to' in message.text else None)
    page.goto(args.origin)
    page.wait_for_load_state('networkidle')
    expect(page.locator('#home-title')).to_be_visible()
    for selector in ['.home-hero header', '.site-header', '.site-footer']:
        assert page.locator(selector).first.evaluate('(e) => getComputedStyle(e).textAlign') == 'center', selector
    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
    page.screenshot(path=str(args.screenshots / 'home-desktop.png'), full_page=True)
    page.mouse.move(100, 100)
    page.mouse.wheel(0, 40)
    page.wait_for_function("document.querySelector('.home-hero').getBoundingClientRect().bottom <= 1")
    page.goto(args.origin)
    page.wait_for_load_state('networkidle')
    page.get_by_role('link', name='向下浏览首页导航').click()
    page.wait_for_function("document.querySelector('.home-hero').getBoundingClientRect().bottom <= 1")
    theme = page.locator('[data-theme-toggle]')
    expect(theme).to_have_count(1)
    for mode in ['light', 'dark', 'system']:
        theme.click()
        expect(theme).to_have_attribute('data-theme-mode', mode)
        assert page.locator(f'[data-theme-icon="{mode}"]:visible').count() == 1
    page.goto(args.origin)
    page.wait_for_load_state('networkidle')

    page.keyboard.press('Tab')
    expect(page.get_by_role('link', name='跳到正文')).to_be_focused()
    page.get_by_role('link', name='发文章教程', exact=True).click()
    expect(page.locator('.article-heading h1')).to_have_text('发文章教程')
    expect(page.locator('.prose')).to_contain_text('克隆个人文章仓库')
    page.goto(args.origin)
    page.get_by_role('link', name='开发进度', exact=True).click()
    expect(page.locator('.article-heading h1')).to_have_text('开发进度')
    page.goto(args.origin)
    page.get_by_role('link', name='阅读文章').click()
    page.wait_for_load_state('networkidle')
    expect(page.get_by_role('heading', name='第一篇文章')).to_be_visible()
    page.get_by_label('搜索文章').fill('没有这样的标题')
    page.get_by_role('button', name='搜索', exact=True).click()
    expect(page.get_by_role('heading', name='没有匹配的文章')).to_be_visible()
    page.get_by_role('link', name='查看全部文章').click()
    page.get_by_role('heading', name='第一篇文章').get_by_role('link').click()
    expect(page.locator('.prose')).to_contain_text('你好，ChuBlog')
    page.screenshot(path=str(args.screenshots / 'article-desktop.png'), full_page=True)
    page.locator('.prose').get_by_role('link', name='阅读下一页').click()
    expect(page.locator('.prose')).to_contain_text('第二页')
    page.locator('.prose').get_by_role('link', name='返回入口').click()
    expect(page.locator('.prose')).to_contain_text('你好，ChuBlog')
    page.goto(args.origin + '/posts/alice/missing')
    expect(page.get_by_role('heading', name='文章不存在')).to_be_visible()

    def login(target, username, secret):
        target.goto(args.origin + '/login')
        target.get_by_label('用户名', exact=True).fill(username)
        target.get_by_label('密码', exact=True).fill(secret)
        target.get_by_role('button', name='登录', exact=True).click()
        expect(target.locator('#account-name')).not_to_be_empty()

    login(page, 'alice', password)
    expect(page.locator('#account-role')).to_have_text('普通用户')
    page.route('**/api/account', lambda route: route.fulfill(status=503, json={'error': '临时读取失败'}))
    page.evaluate("window.dispatchEvent(new PageTransitionEvent('pageshow', {persisted: true}))")
    expect(page.locator('#session-status')).to_be_visible()
    expect(page.locator('#account-name')).not_to_be_empty()
    expect(page.locator('[data-account-link]')).to_have_attribute('href', '/account')
    page.unroute('**/api/account')
    page.locator('#session-retry').click()
    expect(page.locator('#session-status')).to_be_hidden()
    expect(page.locator('#account-role')).to_have_text('普通用户')
    expect(page.locator('#publish-status')).to_contain_text('授予发布权限')
    assert page.locator('.publishing-guide p').first.evaluate('(e) => getComputedStyle(e).fontSize') == '16px'
    expect(page.locator('.publishing-guide code.language-bash span').first).to_be_attached()
    page.locator('.publishing-guide summary').click()
    expect(page.locator('.publishing-guide code.language-toml span').first).to_be_visible()

    page.screenshot(path=str(args.screenshots / 'account-desktop.png'), full_page=True)
    page.get_by_role('button', name='退出登录').click()
    expect(page.locator('#home-title')).to_be_visible()

    # Registration, persistence, password change and subsequent login use real backend sessions.
    page.goto(args.origin + '/login?mode=register')
    page.get_by_label('用户名', exact=True).fill('webreader')
    page.get_by_label('密码', exact=True).fill(password)
    page.get_by_label('确认密码', exact=True).fill(password)
    page.get_by_label('昵称', exact=True).fill('测试昵称')
    page.get_by_role('button', name='创建账户').click()
    expect(page.locator('#account-name')).to_have_text('测试昵称')
    expect(page.locator('#account-username')).to_have_text('(webreader)')
    page.get_by_label('原密码', exact=True).fill(password)
    new_password = password + '-changed'
    page.get_by_label('新密码', exact=True).fill(new_password)
    page.get_by_role('button', name='修改密码', exact=True).click()
    expect(page.locator('.notice')).to_have_text('密码已修改，请重新登录。')
    login(page, 'webreader', new_password)

    admin_context = browser.new_context(viewport={'width': 1440, 'height': 1000}, locale='zh-CN')
    admin = admin_context.new_page()
    admin.on('pageerror', lambda error: failures.append(str(error)))
    login(admin, 'admin', password)
    admin.get_by_role('link', name='用户管理', exact=True).click()
    expect(admin.get_by_role('heading', name='用户权限')).to_be_visible()
    expect(admin.get_by_role('link', name='文章管理', exact=True)).to_be_visible()
    assert admin.locator('.admin-shell .settings-section').evaluate_all('(nodes) => nodes.every(e => getComputedStyle(e).borderTopWidth === "0px")')
    expect(admin.locator('#admin-status')).to_be_hidden()

    alice_form = admin.locator('.admin-user').filter(has=admin.get_by_role('heading', name=re.compile(r'（alice）')))
    expect(alice_form.locator('.form-status')).to_be_hidden()
    alice_form.get_by_role('combobox').select_option('publisher')
    alice_form.get_by_role('button', name='保存授权').click()
    expect(alice_form.get_by_role('status')).to_have_text('授权已保存。')
    admin.goto(args.origin + '/admin/articles')
    own_article = admin.locator('.admin-article').filter(has=admin.get_by_role('heading', name=re.compile('发文章教程')))
    own_hidden = own_article.get_by_role('combobox', name=re.compile('隐藏等级'))
    expect(own_hidden).to_be_enabled()
    own_hidden.select_option('semi')
    own_article.get_by_role('button', name='保存设置', exact=True).click()
    expect(own_article.get_by_role('button', name='保存设置', exact=True)).to_be_enabled()
    admin.reload()
    expect(own_hidden).to_have_value('semi')
    admin_context.close()

    page.set_viewport_size({'width': 390, 'height': 844})
    page.goto(args.origin)
    page.wait_for_load_state('networkidle')
    expect(page.locator('#home-title')).to_be_visible()
    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
    page.screenshot(path=str(args.screenshots / 'home-mobile.png'), full_page=True)
    page.goto(args.origin + '/posts/alice/alice-first')
    expect(page.locator('.prose')).to_contain_text('你好，ChuBlog')
    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
    page.screenshot(path=str(args.screenshots / 'article-mobile.png'), full_page=True)
    assert not failures, failures
    context.close()
    browser.close()
print('PASS: responsive navigation homepage and seeded owner articles, search, multi-page navigation, registration/login, HTTPS Git instructions, password and admin grant UI; no script/CSP errors')
