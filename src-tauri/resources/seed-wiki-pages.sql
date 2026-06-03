-- KingdeeKB 演示种子数据：50 条 wiki_pages
-- 用于阶段五知识图谱开发测试
-- 占位符 __PROJECT__ 在执行时替换为实际项目名

-- 清空已有数据
DELETE FROM wiki_pages WHERE project_id = __PROJECT__;

-- ======== Summary 页面（1-10）========
INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-summary','财务管理系统概述','summary','# 财务管理系统概述

金蝶云星空财务管理系统提供了全面的企业财务管理解决方案，覆盖总账、应收应付、固定资产、出纳管理等核心模块。

## 核心功能
- 多组织财务核算
- 智能记账引擎
- 银企互联
- 电子发票管理','["finance-gl-blueprint","finance-ar-blueprint","finance-ap-blueprint","finance-config"]','["\u8d22\u52a1","\u603b\u8d26","ERP"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'scm-summary','供应链管理系统概述','summary','# 供应链管理系统概述

金蝶云星空供应链管理涵盖采购、销售、库存、VMI等业务，实现从供应商到客户的全链路协同。','["scm-purchase-blueprint","scm-sales-blueprint","scm-inventory-blueprint","scm-fitgap"]','["供应链","采购","销售"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'manufacturing-summary','生产制造系统概述','summary','# 生产制造系统概述

支持多种生产模式：MTS、MTO、ETO、重复生产。集成MES实现车间实时管控。','["mfg-plan-blueprint","mfg-shop-floor-blueprint","mfg-fitgap","decision-bom"]','["制造","MES","生产"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'hr-summary','人力资源系统概述','summary','# 人力资源系统概述

涵盖组织管理、招聘、薪酬、绩效、培训等全模块，支持集团化人力资源管理。','["hr-recruit-blueprint","hr-compensation-blueprint","hr-fitgap"]','["HR","人力","组织"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'crm-summary','客户关系管理系统概述','summary','# 客户关系管理系统概述

实现从线索到回款的全流程客户管理，集成营销、销售、服务三大业务线。','["crm-service-blueprint","crm-marketing-blueprint","crm-fitgap"]','["CRM","客户","营销"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'pm-summary','项目管理系统概述','summary','# 项目管理系统概述

支持从立项到结项的全生命周期管理，包含预算、进度、资源、交付物管理。','["pm-fitgap","decision-approval","wf-config"]','["项目","PMO"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'bi-summary','商业智能分析概述','summary','# 商业智能分析概述

内置报表中心、自助分析、移动BI，支持多维度数据钻取和可视化展现。','["decision-report","finance-gl-blueprint","finance-config"]','["BI","报表","分析"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'bank-summary','银企互联系统概述','summary','# 银企互联系统概述

直连国内外主流银行，实现资金归集、付款指令、余额查询的自动化处理。','["finance-fund-blueprint","finance-ap-blueprint","decision-integration"]','["银企","资金","银行"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'asset-summary','资产管理概述','summary','# 资产管理概述

覆盖固定资产从采购、使用、维护到报废的全生命周期管理。','["finance-gl-blueprint","scm-purchase-blueprint","finance-config"]','["资产","固定"]','{}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'budget-summary','全面预算管理概述','summary','# 全面预算管理概述

支持业务预算、财务预算、资本预算编制、执行控制与分析考核。','["budget-blueprint","finance-gl-blueprint","decision-approval"]','["预算","费控"]','{}','published');

-- ======== Blueprint 页面（11-25）========
INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-gl-blueprint','总账管理模块蓝图','blueprint','# 总账管理模块蓝图

## 关键流程
1. 凭证录入->审核->过账->期末处理
2. 自动转账模板配置
3. 合并报表抵消处理

## 集成点
- 应收模块：凭证自动生成
- 应付模块：付款凭证集成
- 固定资产：折旧凭证','["finance-summary","finance-ar-blueprint","finance-ap-blueprint","decision-accounting"]','["总账","蓝图","GL"]','{"edition":"enterprise","modules":["GL"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-ar-blueprint','应收管理模块蓝图','blueprint','# 应收管理模块蓝图

## 核心功能
客户档案、应收单据、收款核销、账龄分析、信用管理。

## 关键流程
销售出库->应收确认->收款->核销->对账','["finance-summary","scm-sales-blueprint","finance-ar-fitgap","decision-approval"]','["应收","蓝图","AR"]','{"edition":"enterprise","modules":["AR"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-ap-blueprint','应付管理模块蓝图','blueprint','# 应付管理模块蓝图

## 核心功能
供应商档案、采购发票、付款计划、票据管理、预付管理。

## 关键流程
采购入库->发票匹配->付款->核销->对账','["finance-summary","scm-purchase-blueprint","finance-ap-fitgap","finance-fund-blueprint"]','["应付","蓝图","AP"]','{"edition":"enterprise","modules":["AP"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'scm-purchase-blueprint','采购管理模块蓝图','blueprint','# 采购管理模块蓝图

## 关键流程
PR->RFQ->PO->GR->IR->AP','["scm-summary","finance-ap-blueprint","scm-inventory-blueprint","scm-fitgap"]','["采购","蓝图","PUR"]','{"edition":"enterprise","modules":["PUR"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'scm-sales-blueprint','销售管理模块蓝图','blueprint','# 销售管理模块蓝图

## 关键流程
报价->订单->发货->出库->开票->收款','["scm-summary","finance-ar-blueprint","crm-service-blueprint","scm-fitgap"]','["销售","蓝图","SD"]','{"edition":"enterprise","modules":["SD"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'scm-inventory-blueprint','库存管理模块蓝图','blueprint','# 库存管理模块蓝图

## 核心功能
多仓库管理、批次跟踪、序列号管理、盘点、库存预警。','["scm-summary","scm-purchase-blueprint","scm-sales-blueprint","mfg-plan-blueprint"]','["库存","蓝图","INV"]','{"edition":"enterprise","modules":["INV"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'mfg-plan-blueprint','生产计划模块蓝图','blueprint','# 生产计划模块蓝图

## 关键流程
销售预测->MPS->MRP->工单下达->领料->汇报','["manufacturing-summary","mfg-shop-floor-blueprint","scm-inventory-blueprint","decision-bom"]','["计划","蓝图","MRP"]','{"edition":"flagship","modules":["PP"],"status":"draft"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'mfg-shop-floor-blueprint','车间执行模块蓝图','blueprint','# 车间执行模块蓝图

## 关键流程
工单->工序派工->领料->工序汇报->质检->完工入库','["manufacturing-summary","mfg-plan-blueprint","mfg-fitgap"]','["车间","蓝图","MES"]','{"edition":"flagship","modules":["MES"],"status":"draft"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'hr-recruit-blueprint','招聘管理模块蓝图','blueprint','# 招聘管理模块蓝图

招聘需求、渠道管理、简历筛选、面试安排、Offer管理全流程。','["hr-summary","hr-compensation-blueprint","decision-permission"]','["招聘","蓝图","HCM"]','{"edition":"enterprise","modules":["HCM"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'hr-compensation-blueprint','薪酬管理模块蓝图','blueprint','# 薪酬管理模块蓝图

薪资核算、社保公积金、个税计算、薪酬分析、电子工资条。','["hr-summary","hr-recruit-blueprint","finance-gl-blueprint"]','["薪酬","蓝图","HCM"]','{"edition":"enterprise","modules":["HCM"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'crm-service-blueprint','服务管理模块蓝图','blueprint','# 服务管理模块蓝图

服务工单、客户投诉、现场服务、备件管理、服务SLA监控。','["crm-summary","scm-sales-blueprint","crm-fitgap"]','["服务","蓝图","CRM"]','{"edition":"enterprise","modules":["CRM"],"status":"draft"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'crm-marketing-blueprint','市场管理模块蓝图','blueprint','# 市场管理模块蓝图

市场活动、线索分配、客户分级、营销ROI分析。','["crm-summary","crm-service-blueprint","bi-summary"]','["市场","蓝图","CRM"]','{"edition":"enterprise","modules":["CRM"],"status":"draft"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'budget-blueprint','预算编制模块蓝图','blueprint','# 预算编制模块蓝图

预算模板、自下而上编制、多版本管理、预算审批流程。','["budget-summary","finance-gl-blueprint","decision-approval"]','["预算","蓝图","BGT"]','{"edition":"enterprise","modules":["BGT"],"status":"approved"}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-fund-blueprint','资金管理模块蓝图','blueprint','# 资金管理模块蓝图

资金计划、银企直连、票据管理、内部借贷、现金流预测。','["bank-summary","finance-ap-blueprint","finance-gl-blueprint","finance-config"]','["资金","蓝图","TR"]','{"edition":"flagship","modules":["TR"],"status":"draft"}','published');

-- ======== Fit-Gap 页面（26-35）========
INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-ar-fitgap','应收管理 Fit-Gap','fitgap','# 应收管理 Fit-Gap

差距项：
- 多币种收款自动识别 -> 需二次开发
- 银企回单自动匹配 -> 需集成平台','["finance-ar-blueprint","finance-summary","finance-config","decision-integration"]','["应收","Fit-Gap","AR"]','{"module":"AR","gaps":["多币种识别","银企回单"],"decisions":["二次开发"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-ap-fitgap','应付管理 Fit-Gap','fitgap','# 应付管理 Fit-Gap

差距项：
- 供应商门户协同 -> 需实施协同平台','["finance-ap-blueprint","scm-purchase-blueprint","decision-approval"]','["应付","Fit-Gap","AP"]','{"module":"AP","gaps":["供应商门户"],"decisions":["实施协同平台"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'scm-fitgap','供应链管理 Fit-Gap','fitgap','# 供应链管理 Fit-Gap

差距项：
- 供应商评分自动化 -> 需配置评分模型
- 运输管理模块缺失 -> 建议独立TMS','["scm-summary","scm-purchase-blueprint","scm-sales-blueprint","scm-inventory-blueprint"]','["供应链","Fit-Gap","SCM"]','{"module":"SCM","gaps":["供应商评分","TMS"],"decisions":["配置评分","TMS选型"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'mfg-fitgap','生产管理 Fit-Gap','fitgap','# 生产管理 Fit-Gap

差距项：
- 高级排程APS -> 需集成专业APS
- 数字孪生 -> 远期规划','["manufacturing-summary","mfg-plan-blueprint","mfg-shop-floor-blueprint","decision-bom"]','["制造","Fit-Gap","MFG"]','{"module":"MFG","gaps":["APS","数字孪生"],"decisions":["集成APS","远期规划"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'hr-fitgap','人力管理 Fit-Gap','fitgap','# 人力管理 Fit-Gap

差距项：
- 绩效管理需深度定制
- 人才盘点模块缺失','["hr-summary","hr-recruit-blueprint","hr-compensation-blueprint"]','["人力","Fit-Gap","HCM"]','{"module":"HCM","gaps":["绩效定制","人才盘点"],"decisions":["二次开发"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'crm-fitgap','CRM 管理 Fit-Gap','fitgap','# CRM Fit-Gap

差距项：
- 自动化营销引擎 -> 建议集成营销自动化平台','["crm-summary","crm-service-blueprint","crm-marketing-blueprint"]','["CRM","Fit-Gap","CRM"]','{"module":"CRM","gaps":["营销自动化"],"decisions":["集成平台"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'pm-fitgap','项目管理 Fit-Gap','fitgap','# 项目管理 Fit-Gap

差距项：
- 关键链法不支持 -> 需二次开发','["pm-summary","decision-approval","wf-config"]','["项目","Fit-Gap","PM"]','{"module":"PM","gaps":["关键链"],"decisions":["二次开发"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-fitgap','财务管理 Fit-Gap','fitgap','# 财务管理 Fit-Gap

差距项：
- 会计准则差异需配置
- 合并报表自动化不足','["finance-summary","finance-gl-blueprint","finance-config","decision-accounting"]','["财务","Fit-Gap","FIN"]','{"module":"FIN","gaps":["准则差异","合并报表"],"decisions":["配置优化"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'it-fitgap','IT 架构 Fit-Gap','fitgap','# IT 架构 Fit-Gap

差距项：
- 数据迁移工具需定制开发','["decision-integration","decision-permission","decision-migration"]','["IT","Fit-Gap","架构"]','{"module":"IT","gaps":["数据迁移工具"],"decisions":["定制开发"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'qa-fitgap','质量管理 Fit-Gap','fitgap','# 质量管理 Fit-Gap

差距项：
- 8D报告模块缺失 -> 二次开发','["manufacturing-summary","mfg-shop-floor-blueprint","decision-bom"]','["质量","Fit-Gap","QM"]','{"module":"QM","gaps":["8D报告"],"decisions":["二次开发"]}','published');

-- ======== Decision 页面（36-45）========
INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-accounting','会计科目体系设计决策','decision','# 会计科目体系设计决策

## 决策
采用6位统一科目表，辅助核算段支持自定义。

## 影响
涉及总账、应收、应付模块，需数据迁移脚本处理历史数据。','["finance-gl-blueprint","finance-ar-blueprint","finance-ap-blueprint","decision-migration"]','["科目","决策","核算"]','{"decision_date":"2026-03-15","decision_maker":"财务组","alternatives":["4位","8位"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-approval','审批流程设计决策','decision','# 审批流程设计决策

## 方案
基于工作流引擎配置审批流，支持动态审批人。相比硬编码更具灵活性。','["wf-config","pm-summary","budget-blueprint","finance-ar-blueprint"]','["审批","决策","工作流"]','{"decision_date":"2026-02-20","decision_maker":"平台组","alternatives":["硬编码","审批矩阵"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-coding','编码规则设计决策','decision','# 编码规则设计决策

采用弹性编码方案，支持按业务类型自定义编码段。适用于物料编码、客户编码、会计科目编码。','["finance-config","scm-purchase-blueprint","scm-inventory-blueprint"]','["编码","决策","规则"]','{"decision_date":"2026-01-10","decision_maker":"实施组","alternatives":["固定长度","流水号"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-permission','权限体系设计决策','decision','# 权限体系设计决策

采用RBAC+行级数据权限双模型，满足集团企业复杂权限管控需求。','["hr-recruit-blueprint","hr-summary","it-fitgap"]','["权限","决策","RBAC"]','{"decision_date":"2026-01-05","decision_maker":"安全组","alternatives":["仅RBAC","ABAC"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-integration','集成方案设计决策','decision','# 集成方案设计决策

采用事件驱动+API网关的集成架构，适用于ERP、OA、WMS、MES间的数据同步。','["bank-summary","it-fitgap","finance-ar-fitgap","finance-config"]','["集成","决策","API"]','{"decision_date":"2026-02-28","decision_maker":"架构组","alternatives":["ESB","点对点"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-migration','数据迁移策略决策','decision','# 数据迁移策略决策

分批次迁移：先静态数据（科目、客户、供应商），后动态数据（余额、未清单据）。采用ETL+校验脚本双保险。','["it-fitgap","decision-accounting","finance-config"]','["迁移","决策","数据"]','{"decision_date":"2026-03-01","decision_maker":"数据组","alternatives":["全量一次性","按模块"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-report','报表体系设计决策','decision','# 报表体系设计决策

统一报表平台：BI分析+固定报表+自助查询。覆盖管理报表、财务报表、业务报表、监管报表。','["bi-summary","finance-gl-blueprint","finance-config"]','["报表","决策","BI"]','{"decision_date":"2026-03-20","decision_maker":"报表组","alternatives":["仅固定报表","仅BI"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-alert','预警机制设计决策','decision','# 预警机制设计决策

采用规则引擎+消息推送，支持阈值预警、趋势预警、异常检测。','["finance-config","scm-inventory-blueprint","wf-config"]','["预警","决策","规则"]','{"decision_date":"2026-03-25","decision_maker":"技术组","alternatives":["定时轮询","实时计算"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-bom','BOM 管理设计决策','decision','# BOM 管理设计决策

多视图BOM：设计BOM、制造BOM、成本BOM统一管理。支持工程变更的BOM版本追溯。','["mfg-plan-blueprint","mfg-fitgap","manufacturing-summary"]','["BOM","决策","版本"]','{"decision_date":"2026-04-01","decision_maker":"产品组","alternatives":["单视图BOM","按订单BOM"]}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'decision-barcode','条码管理设计决策','decision','# 条码管理设计决策

采用GS1-128编码标准，支持一维码+二维码双模式。应用于仓库收发货、车间物料流转、资产盘点。','["scm-inventory-blueprint","mfg-shop-floor-blueprint","asset-summary"]','["条码","决策","GS1"]','{"decision_date":"2026-04-05","decision_maker":"物流组","alternatives":["仅二维码","RFID"]}','published');

-- ======== Config 页面（46-50）========
INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'finance-config','财务系统参数配置','config','# 财务系统参数配置

## 关键参数
- 会计期间：自然月
- 本位币：CNY
- 汇率类型：月平均汇率
- 凭证类型：记、收、付、转','["finance-summary","finance-gl-blueprint","decision-accounting","decision-report"]','["配置","财务","参数"]','{"module":"FIN","system_path":"系统管理/财务参数","parameters":{"period":"自然月","currency":"CNY"}}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'wf-config','工作流配置说明','config','# 工作流配置说明

1. 定义流程模板（并行/串行/会签）
2. 设置审批节点条件
3. 绑定业务单据

常用流程：采购审批、付款审批、请假审批、合同审批','["decision-approval","pm-summary","finance-ap-blueprint"]','["配置","工作流","审批"]','{"module":"BPM","system_path":"流程中心/流程设计","parameters":{}}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'print-config','打印模板配置','config','# 打印模板配置

支持可视化设计器配置套打模板。支持凭证打印、单据打印、报表打印。','["finance-gl-blueprint","finance-config","bi-summary"]','["配置","打印","套打"]','{"module":"PLT","system_path":"系统管理/打印模板","parameters":{}}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'bpm-config','业务流程配置','config','# 业务流程配置

通过流程配置平台自定义业务流转规则。典型配置：销售->生产->采购联动，审批后自动下推。','["wf-config","scm-sales-blueprint","mfg-plan-blueprint"]','["配置","流程","BPM"]','{"module":"BPM","system_path":"流程中心/业务流","parameters":{}}','published');

INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'msg-config','消息平台配置','config','# 消息平台配置

## 通知方式
- 系统内部消息、邮件通知、企业微信/钉钉推送

## 触发事件
任务分配、审批超时、库存预警、到期提醒','["decision-alert","wf-config","scm-inventory-blueprint"]','["配置","消息","通知"]','{"module":"MSG","system_path":"系统管理/消息配置","parameters":{}}','published');

-- 第 50 条：研发管理概述
INSERT INTO wiki_pages (project_id, slug, title, page_type, content, wikilinks, tags, page_metadata, page_status) VALUES
(__PROJECT__,'rd-summary','研发管理系统概述','summary','# 研发管理系统概述

覆盖产品研发全生命周期管理，包括需求管理、项目规划、任务跟踪、版本发布等核心功能。

## 核心功能
- 产品需求管理
- 研发项目规划
- 代码版本管理集成
- 测试用例管理
- 发布流程管理','["pm-summary","decision-approval","wf-config","msg-config"]','["研发","产品","RD"]','{}','published');
