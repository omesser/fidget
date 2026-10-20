#!/usr/bin/env ruby
# frozen_string_literal: true

# Check the Homebrew cask against its Release disk image, or rewrite the
# stanzas that track that image: version, checksum, url, app, quit, zap,
# caveat, and livecheck.
#   scripts/homebrew-cask.rb verify | bump [--check] <tag>

require "digest"
require "json"
require "open3"
require "tempfile"
require "tmpdir"

ROOT = File.expand_path("..", __dir__)
CASK_PATH = File.join(ROOT, "packaging/homebrew/Casks/fidget.rb")
REPO = "omesser/fidget"

# Leaf under Application Support for each bundle id this repo ships. An id
# with no entry fails the check instead of guessing a directory to delete.
DATA_DIR_LEAF = {
  "dev.omesser.fidget" => "fidget",
}.freeze

Facts = Struct.new(
  :tag, :version, :sha256, :url, :asset_name, :prefix, :app,
  :bundle_id, :display_name, :data_leaf, :bytes,
  keyword_init: true
)

# livecheck is two stanzas. Recording them here keeps a wrong strategy from
# loading as an unchecked block.
class LivecheckCapture
  def url(value = nil)
    return @url if value.nil?

    @url = value
  end

  def strategy(value = nil)
    return @strategy if value.nil?

    @strategy = value
  end

  def method_missing(name, *)
    raise "unsupported livecheck stanza #{name}"
  end

  def respond_to_missing?(*)
    false
  end
end

# A Homebrew cask is Ruby. This records the stanzas the checker understands
# and rejects the rest, so a new stanza cannot pass unexamined.
class CaskCapture
  attr_reader :token, :names, :quit, :depends, :livecheck_url, :livecheck_strategy

  def initialize
    @names = []
    @depends = []
  end

  def cask(token, &block)
    @token = token
    instance_eval(&block)
  end

  def version(value = nil)
    return @version if value.nil?

    @version = value
  end

  def sha256(value = nil)
    return @sha256 if value.nil?

    @sha256 = value
  end

  def url(value = nil)
    return @url if value.nil?

    @url = value
  end

  def name(value)
    @names << value
  end

  def desc(value = nil)
    return @desc if value.nil?

    @desc = value
  end

  def homepage(value = nil)
    return @homepage if value.nil?

    @homepage = value
  end

  def livecheck(&block)
    raise "livecheck already recorded" if @livecheck_url
    raise "livecheck needs a block" unless block

    inner = LivecheckCapture.new
    inner.instance_eval(&block)
    @livecheck_url = inner.url
    @livecheck_strategy = inner.strategy
  end

  def depends_on(req = nil, arch: nil, macos: nil)
    @depends << { req: req, arch: arch, macos: macos }
  end

  def app(value = nil)
    return @app if value.nil?

    raise "only one app stanza is checked" if @app

    @app = value
  end

  def uninstall(quit: nil)
    return @quit if quit.nil?

    @quit = quit
  end

  def zap(trash: nil)
    return @zap if trash.nil?

    @zap = trash
  end

  def caveats(text = nil, &block)
    return @caveats if text.nil? && block.nil?

    raise "caveats must be a string so the checker can read it" if block || text.nil?

    @caveats = text
  end

  def method_missing(name, *)
    raise "unsupported cask stanza #{name}"
  end

  def respond_to_missing?(*)
    false
  end
end

def die(message)
  warn message
  exit 1
end

def usage
  warn <<~MSG
    usage: scripts/homebrew-cask.rb verify
           scripts/homebrew-cask.rb bump [--check] <tag>
  MSG
  exit 2
end

def seven_zip
  %w[7z 7zz].find do |bin|
    ENV.fetch("PATH").split(File::PATH_SEPARATOR).any? do |dir|
      File.executable?(File.join(dir, bin))
    end
  end || die("7z not found. Install: sudo apt-get install p7zip-full")
end

def curl(url, dest = nil)
  args = ["curl", "-fsSL", "--retry", "3", "--retry-all-errors", "-H", "User-Agent: fidget-homebrew-cask"]
  token = ENV["GITHUB_TOKEN"] || ENV["GH_TOKEN"]
  args.push("-H", "Authorization: Bearer #{token}") if token && !token.empty?
  args.push("-H", "Accept: application/vnd.github+json") if dest.nil?
  if dest
    args.push("-o", dest, url)
  else
    args.push(url)
  end
  out, err, status = Open3.capture3(*args)
  return out if status.success?

  detail = err.strip.empty? ? out.strip : err.strip
  die("curl #{url} failed: #{detail}")
end

def release(tag)
  body = curl("https://api.github.com/repos/#{REPO}/releases/tags/#{tag}")
  JSON.parse(body)
rescue JSON::ParserError
  die("GitHub release #{tag} was not JSON")
end

# Same endpoint Homebrew's :github_latest strategy reads. It is the marked
# Latest release, so drafts and prereleases are already excluded.
def latest_stable_tag
  body = curl("https://api.github.com/repos/#{REPO}/releases/latest")
  parsed = JSON.parse(body)
  die("GitHub latest is a prerelease") if parsed["prerelease"]
  die("GitHub latest is a draft") if parsed["draft"]
  parsed.fetch("tag_name")
rescue JSON::ParserError
  die("GitHub latest release was not JSON")
end

def download(url)
  file = Tempfile.new(["fidget-", ".dmg"])
  file.close
  curl(url, file.path)
  [file, Digest::SHA256.file(file.path).hexdigest, File.size(file.path)]
end

def plist_xml(dmg_path, app)
  listing, err, status = Open3.capture3(seven_zip, "l", "-ba", dmg_path)
  die("7z l failed: #{err}") unless status.success?

  paths = listing.lines.filter_map { |line| line[/(\S+\/Contents\/Info\.plist)\s*$/, 1] }
  needle = "#{app}/Contents/Info.plist"
  path = paths.find { |entry| entry.end_with?(needle) }
  die("disk image has no #{needle} (found #{paths.join(', ')})") unless path

  xml, err, status = Open3.capture3(seven_zip, "e", "-so", dmg_path, path)
  die("7z e failed: #{err}") unless status.success?
  xml
end

def plist_string(xml, key)
  xml[%r{<key>#{Regexp.escape(key)}</key>\s*<string>([^<]*)</string>}, 1] ||
    die("Info.plist has no string #{key}")
end

def normalize_tag(arg)
  tag = arg.strip
  tag.start_with?("v") ? tag : "v#{tag}"
end

def facts_for(tag)
  tag = normalize_tag(tag)
  version = tag.delete_prefix("v")
  die("tag #{tag} is not v plus a version") unless tag == "v#{version}"

  asset = release(tag).fetch("assets").select { |entry| entry.fetch("name").end_with?("_aarch64.dmg") }
  die("expected one *_aarch64.dmg on #{tag}, found #{asset.map { |entry| entry['name'] }.join(', ')}") unless asset.size == 1

  asset = asset.fetch(0)
  name = asset.fetch("name")
  suffix = "_#{version}_aarch64.dmg"
  die("#{name} does not end with #{suffix}") unless name.end_with?(suffix)

  prefix = name.delete_suffix(suffix)
  die("asset prefix #{prefix.inspect} is not a single filename segment") if prefix.empty? || prefix.match?(%r{[/\\"]})

  url = asset.fetch("browser_download_url")
  expected = "https://github.com/#{REPO}/releases/download/#{tag}/#{name}"
  die("browser_download_url #{url} is not #{expected}") unless url == expected

  app = "#{prefix}.app"
  file, sha, bytes = download(url)
  begin
    digest = asset["digest"].to_s
    if digest.start_with?("sha256:") && digest.delete_prefix("sha256:").downcase != sha
      die("GitHub digest #{digest} does not match downloaded sha256 #{sha}")
    end

    xml = plist_xml(file.path, app)
    bundle_id = plist_string(xml, "CFBundleIdentifier")
    display = plist_string(xml, "CFBundleDisplayName")
    leaf = DATA_DIR_LEAF[bundle_id] || die("no Application Support leaf for bundle id #{bundle_id}")
    %w[CFBundleShortVersionString CFBundleVersion].each do |key|
      found = plist_string(xml, key)
      die("#{key} is #{found}, tag version is #{version}") unless found == version
    end
    die("CFBundleName is #{plist_string(xml, 'CFBundleName')}, asset prefix is #{prefix}") unless plist_string(xml, "CFBundleName") == prefix
    # Cargo names the bin in lowercase. The asset prefix is the product name.
    executable = plist_string(xml, "CFBundleExecutable")
    unless [prefix, prefix.downcase].uniq.include?(executable)
      die("CFBundleExecutable is #{executable}, asset prefix is #{prefix}")
    end

    Facts.new(
      tag: tag, version: version, sha256: sha, url: url, asset_name: name,
      prefix: prefix, app: app, bundle_id: bundle_id, display_name: display,
      data_leaf: leaf, bytes: bytes
    )
  ensure
    file.unlink
  end
end

def zap_paths(leaf)
  ["~/Library/Application Support/#{leaf}"]
end

def zap_source(facts)
  lines = zap_paths(facts.data_leaf).map { |path| %(    "#{path}",) }
  "  zap trash: [\n#{lines.join("\n")}\n  ]"
end

def livecheck_source
  <<~RUBY.gsub(/^/, "  ").chomp
    livecheck do
      url :url
      strategy :github_latest
    end
  RUBY
end

def ensure_livecheck(text)
  block = livecheck_source
  pattern = /^  livecheck do\n(?:    .+\n)*  end\n/
  if text.match?(pattern)
    text.sub(pattern, "#{block}\n")
  else
    found = text.sub(/^(  homepage "[^"]+"\n)/, "\\1\n#{block}\n")
    die("cask has no homepage line to place livecheck after") if found == text
    found
  end
end

def caveat_source(display)
  body = <<~TEXT
    This build installs #{display}. It is not signed. Dismiss the Gatekeeper
    dialog, then System Settings → Privacy & Security → Open Anyway.
  TEXT
  indented = body.lines.map { |line| "    #{line}" }.join
  "  caveats <<~EOS\n#{indented}  EOS"
end

def apply_facts(text, facts)
  text = ensure_livecheck(text)
  zap_pattern = %r{^  zap trash: (?:"~/Library/Application Support/[^"]+"|\[\n(?:    "~/Library/Application Support/[^"]+",\n)+  \])$}
  replacements = [
    [/^  version "[^"]*"$/, %(  version "#{facts.version}")],
    [/^  sha256 "[0-9a-f]{64}"$/, %(  sha256 "#{facts.sha256}")],
    [%r{^  url "https://github\.com/omesser/fidget/releases/download/v#\{version\}/[^"]+_#\{version\}_aarch64\.dmg"$},
     %(  url "https://github.com/omesser/fidget/releases/download/v\#{version}/#{facts.prefix}_\#{version}_aarch64.dmg")],
    [/^  app "[^"]+\.app"$/, %(  app "#{facts.app}")],
    [/^  uninstall quit: "[^"]+"$/, %(  uninstall quit: "#{facts.bundle_id}")],
    [zap_pattern, zap_source(facts)],
    [/^  caveats <<~EOS\n(?:.*\n)*?  EOS$/, caveat_source(facts.display_name)],
  ]
  replacements.reduce(text) do |current, (regex, line)|
    found = current.scan(regex).size
    die("expected 1 cask match for #{regex.inspect}, found #{found}") unless found == 1
    current.gsub(regex) { line }
  end
end

def load_cask(source, path)
  capture = CaskCapture.new
  capture.instance_eval(source, path, 1)
  capture
end

def expect(label, actual, expected)
  die("#{label} is #{actual.inspect}, Release has #{expected.inspect}") unless actual == expected
end

def check_shape(capture, facts)
  expect("cask token", capture.token, "fidget")
  expect("version", capture.version, facts.version)
  expect("sha256", capture.sha256, facts.sha256)
  expect("url", capture.url, facts.url)
  expect("app", capture.app, facts.app)
  expect("uninstall quit", capture.quit, facts.bundle_id)
  expect("zap", capture.zap, zap_paths(facts.data_leaf))
  expect("livecheck url", capture.livecheck_url, :url)
  expect("livecheck strategy", capture.livecheck_strategy, :github_latest)
  expect("first name", capture.names.first, "Fidget")
  die("cask names #{capture.names.inspect} omit #{facts.display_name}") unless capture.names.include?(facts.display_name)
  desc = capture.desc.to_s
  die("desc must be one non-empty line under 80 characters") unless !desc.empty? && desc.length < 80 && !desc.end_with?(".") && desc[0] == desc[0].upcase && !desc.include?("\n")
  expect("homepage", capture.homepage, "https://github.com/#{REPO}")
  die("cask is missing depends_on arch: :arm64") unless capture.depends.any? { |dep| dep[:arch] == :arm64 }
  die("cask is missing depends_on :macos") unless capture.depends.any? { |dep| dep[:req] == :macos }
  caveat = capture.caveats.to_s
  die("caveat does not name #{facts.display_name}") unless caveat.include?(facts.display_name)
  die("caveat does not say the build is unsigned") unless caveat.include?("not signed")
end

def show_diff(old, new)
  Dir.mktmpdir do |dir|
    before = File.join(dir, "cask.rb")
    after = File.join(dir, "bumped.rb")
    File.write(before, old)
    File.write(after, new)
    system("diff", "-u", before, after)
  end
end

def require_unchanged(text, facts)
  updated = apply_facts(text, facts)
  return if updated == text

  warn "cask does not match #{facts.tag}"
  show_diff(text, updated)
  exit 1
end

def report(facts)
  puts "release #{facts.tag} asset #{facts.asset_name}"
  puts "sha256 #{facts.sha256} matches download (#{facts.bytes} bytes)"
  puts "app #{facts.app} bundle id #{facts.bundle_id}"
  puts "zap #{zap_paths(facts.data_leaf).join(', ')}"
end

def verify
  text = File.read(CASK_PATH)
  capture = load_cask(text, CASK_PATH)
  die("cask has no version") if capture.version.to_s.empty?

  facts = facts_for("v#{capture.version}")
  check_shape(capture, facts)
  latest = latest_stable_tag
  die("livecheck latest is #{latest}, cask pins #{facts.tag}") unless latest == facts.tag
  require_unchanged(text, facts)
  report(facts)
  puts "livecheck latest #{latest}"
  puts "cask matches #{facts.tag}"
end

def bump(tag, check:)
  text = File.read(CASK_PATH)
  facts = facts_for(tag)
  updated = apply_facts(text, facts)
  check_shape(load_cask(updated, CASK_PATH), facts)
  if check
    require_unchanged(text, facts)
    report(facts)
    puts "bump --check: cask matches #{facts.tag}"
    return
  end
  if updated == text
    report(facts)
    puts "cask already matches #{facts.tag}"
    return
  end
  File.write(CASK_PATH, updated)
  report(facts)
  puts "updated #{CASK_PATH} to #{facts.tag}"
end

def main
  case ARGV[0]
  when "verify"
    die("verify takes no arguments") if ARGV.length != 1
    verify
  when "bump"
    check = ARGV[1] == "--check"
    tag = check ? ARGV[2] : ARGV[1]
    usage if tag.nil? || ARGV.length != (check ? 3 : 2)
    bump(tag, check: check)
  else
    usage
  end
end

main
