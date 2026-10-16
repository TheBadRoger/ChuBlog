#!/usr/bin/env python3
"""Verify the deployed Astro frontend against the real API. Password is read from stdin.
Requires Python Playwright and Chromium. Invoked optionally by smoke.py --browser-test.
"""
import argparse
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
    expect(page.get_by_role('heading', name='个人博客正在施工中')).to_be_visible()
    for selector in ['.construction', '.construction h1', '.construction-progress h2', '.construction-progress p', '.site-header', '.site-footer']:
        assert page.locator(selector).first.evaluate('(e) => getComputedStyle(e).textAlign') == 'center', selector
    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
    page.screenshot(path=str(args.screenshots / 'home-desktop.png'), full_page=True)
    page.keyboard.press('Tab')
    expect(page.get_by_role('link', name='跳到正文')).to_be_focused()
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
    page.goto(args.origin + '/posts/alice~missing')
    expect(page.get_by_role('heading', name='文章不存在')).to_be_visible()

    def login(target, username, secret):
        target.goto(args.origin + '/login')
        target.get_by_label('用户名', exact=True).fill(username)
        target.get_by_label('密码', exact=True).fill(secret)
        target.get_by_role('button', name='登录', exact=True).click()
        expect(target.locator('#account-name')).to_have_text(username)

    login(page, 'alice', password)
    expect(page.locator('#account-role')).to_have_text('普通用户')
    page.get_by_role('button', name='保存公钥').click()
    expect(page.locator('#key-form .form-status')).to_have_text('公钥已保存。发布仍需站长授权。')
    page.screenshot(path=str(args.screenshots / 'account-desktop.png'), full_page=True)
    page.get_by_role('button', name='退出登录').click()
    expect(page.get_by_role('heading', name='个人博客正在施工中')).to_be_visible()

    # Registration, persistence, password change and subsequent login use real backend sessions.
    page.goto(args.origin + '/login?mode=register')
    page.get_by_label('用户名', exact=True).fill('web-reader')
    page.get_by_label('密码', exact=True).fill(password)
    page.get_by_label('确认密码', exact=True).fill(password)
    page.get_by_role('button', name='创建账户').click()
    expect(page.locator('#account-name')).to_have_text('web-reader')
    page.get_by_label('原密码', exact=True).fill(password)
    new_password = password + '-changed'
    page.get_by_label('新密码', exact=True).fill(new_password)
    page.get_by_role('button', name='修改密码', exact=True).click()
    expect(page.locator('.notice')).to_have_text('密码已修改，请重新登录。')
    login(page, 'web-reader', new_password)

    admin_context = browser.new_context(viewport={'width': 1440, 'height': 1000}, locale='zh-CN')
    admin = admin_context.new_page()
    admin.on('pageerror', lambda error: failures.append(str(error)))
    login(admin, 'admin', password)
    expect(admin.get_by_role('heading', name='发布授权')).to_be_visible()
    alice_form = admin.locator('.admin-user').filter(has=admin.get_by_role('heading', name='alice', exact=True))
    alice_form.get_by_label('alice 的角色').select_option('publisher')
    alice_form.get_by_role('button', name='保存授权').click()
    expect(alice_form.get_by_role('status')).to_have_text('授权已保存。')
    admin_context.close()

    page.set_viewport_size({'width': 390, 'height': 844})
    page.goto(args.origin)
    page.wait_for_load_state('networkidle')
    expect(page.get_by_role('heading', name='个人博客正在施工中')).to_be_visible()
    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
    page.screenshot(path=str(args.screenshots / 'home-mobile.png'), full_page=True)
    page.goto(args.origin + '/posts/alice~alice-first')
    expect(page.locator('.prose')).to_contain_text('你好，ChuBlog')
    assert page.evaluate('document.documentElement.scrollWidth <= innerWidth')
    page.screenshot(path=str(args.screenshots / 'article-mobile.png'), full_page=True)
    assert not failures, failures
    context.close()
    browser.close()
print('PASS: centered responsive homepage, search, multi-page navigation, registration/login, SSH key, password and admin grant UI; no script/CSP errors')
